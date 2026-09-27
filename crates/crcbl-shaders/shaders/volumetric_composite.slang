// crcbl-targets: spirv, wgsl, msl, dxil

// The other half of `volumetric.slang`: one fullscreen pass that puts the
// froxel column onto the frame — and, on a frame with an atmosphere, the air in
// front of every surface under it.
//
// ```text
// behind = scene * air.a + air.rgb                                  (aerial)
// out    = behind * (prefix.a * partial.a) + prefix.rgb + prefix.a * partial.rgb
// ```
//
// # Two volumes, the air first
//
// `air` is `crcbl_shaders::atmosphere::AerialView` read along the pixel's ray
// at the surface's distance — Hillaire's `float4(in-scatter, mean
// transmittance)`, marched on the host beside the sky-view LUT and uploaded as
// the `aerial` buffer below. It is composed **before** the local column,
// `((scene * T_A + S_A) * T_F) + S_F`, because the local fog is the near medium:
// its glow is between the eye and the kilometres of air behind it, and must
// not be attenuated by them.
//
// The aerial term is **skipped where the depth is still the clear value**.
// `sky.slang`'s `atmosphere_radiance` already integrates the whole of a view
// ray that leaves the scene, first kilometres included, so charging a sky pixel
// the air in front of it again would fog the sky twice. The local column does
// fog the sky, on purpose and as it always has: the sky-view LUT knows nothing
// of the scene's own fog.
//
// Each half has its own switch, so either runs alone: `aerial_params.w` for the
// air, and `fog_params.w` for the column — zero, the value every frame before
// the air existed wrote, composes the column, and
// `crcbl_shaders::volumetric::LOCAL_COLUMN_OFF` skips it on a frame whose
// scatter and scan did not run. A frame with no atmosphere writes zeroes into
// `aerial_params`, so it composes the bytes it always composed.
//
// `prefix` is what the column in front of this pixel's slice did — the exclusive
// scan `integrateMain` wrote — and `partial` is the part of that slice the pixel
// actually sits in, integrated here. Splitting it that way is what keeps the
// answer off the slice boundaries: an inclusive scan read straight out would
// charge every pixel for the whole of its own slice, and at the far end of an
// exponential split that slice is hundreds of units deep.
//
// # A pass of its own, rather than a term in `mesh.slang`
//
// `docs/notes/rendering.md` argues both halves. The storage-buffer budget is
// one: `crcbl_hal::PORTABLE_STORAGE_BUFFERS_PER_STAGE` is a sum over a whole
// pipeline layout, and the mesh layout's are visible to the vertex stage on the
// path that has no mesh shader. The other is ordering — `mesh.slang` fogs the
// radiance it finishes and `ssr_blur.slang` adds the reflection afterwards, so
// a term added there arrives in front of the reflection rather than over it.
//
// # What this pass does *not* do
//
// It reads one froxel per pixel and does not filter between them. The column is
// cut on the tile grid, so two pixels in different tiles at the same depth read
// prefixes computed along two different tile-centre rays — visible as tile-sized
// steps where the medium is thick. The froxel column's rung 3 is where a filtered
// lookup buys that back, and it is the rung that needs a 3D image.

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
/// `crcbl_shaders::light::SLICE_RATIO`. See `volumetric.slang` on why this is a
/// constant and a multiply chain rather than a `pow`.
static const float CLUSTER_SLICE_RATIO = 1.4677993;

/// The reversed-Z far plane in NDC, matching `crcbl_hal::depth::FAR` — and the
/// value the scene depth carries where nothing was drawn.
static const float DEPTH_FAR = 0.0;

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
/// `crcbl_shaders::mesh::SHADOW_CASCADES`.
///
/// Read here only to size the block's matrix array: nothing in this file walks
/// a cascade, because `scatterMain` already did it once per froxel and left the
/// answer in `lighting`.
static const uint SHADOW_CASCADES = 2;

/// Tiles in the atlas's light region, matching
/// `crcbl_shaders::mesh::SHADOW_LIGHT_TILES`. Read here for the same reason
/// as the count above: it sizes the block's second matrix array.
static const uint SHADOW_LIGHT_TILES = 14;

/// Slots the atlas has, matching `crcbl_shaders::mesh::SHADOW_ATLAS_TILES`.
/// Read here for the same reason as the two counts above: it sizes the block's
/// row of atlas rectangles.
static const uint SHADOW_ATLAS_TILES = 16;

/// The block `volumetric.slang` declares, bound here unchanged so a pixel's
/// froxel and the froxel the column was written into are one arithmetic.
struct VolumetricParams
{
    float4x4 inverse_view_proj;
    float4 eye;
    float4 depth_row;
    float4 fog_params;
    float4 fog_color;
    /// Towards the sun in `xyz`, the anisotropy in `w`.
    float4 sun_direction;
    /// What the medium scatters out of the sun per unit length, in `rgb`, and
    /// out of a punctual light in `w`. Declared and not read: the punctual
    /// term arrives in `lighting`, already scaled.
    float4 sun_radiance;
    /// World → cascade `i`'s shadow clip. Declared and not read — see
    /// `SHADOW_CASCADES` above; the block is one layout, so both files carry
    /// every row of it whether they use it or not.
    float4x4 shadow_view_proj[SHADOW_CASCADES];
    /// How far each cascade reaches. Declared and not read, for the same
    /// reason.
    float4 cascade_far;
    /// The atlas's texel size and `mesh.slang`'s receiver biases. Declared and
    /// not read, for the same reason.
    float4 shadow_params;
    uint grid_x;
    uint grid_y;
    uint slices;
    uint tile_pixels;
    uint viewport_x;
    uint viewport_y;
    uint froxel_count;
    uint pad0;
    /// World → light tile `i`'s shadow clip. Declared and not read, for
    /// `shadow_view_proj`'s reason; sized by `SHADOW_LIGHT_TILES` above.
    float4x4 light_view_proj[SHADOW_LIGHT_TILES];
    /// Where atlas slot `i`'s map is in the shadow atlas: a scale into the image
    /// in `xy` and an offset in `zw`, the same rows
    /// `FrameUniforms::shadow_atlas_rect` carries. `atlas_rect` reads it and
    /// `mesh.slang` is where the layout is argued.
    ///
    /// **After the light matrices**, so no member before it moved when it
    /// arrived.
    float4 shadow_atlas_rect[SHADOW_ATLAS_TILES];
    /// The unit direction **towards** the sun the aerial LUT was marched for
    /// in `xyz`, and in `w` whether this frame composes the air at all —
    /// `crcbl_shaders::sky::ATMOSPHERE_ON` or `ATMOSPHERE_OFF`. Read by the
    /// composite alone.
    ///
    /// **Last in the block**, so no member before it moved when it arrived,
    /// and a frame with no atmosphere writes the zeroes that leave it off.
    float4 aerial_params;
};

[[vk::binding(0, 0)]]
ConstantBuffer<VolumetricParams> params D3D12_REGISTER(b0, space0);

/// The depth prepass's output, fetched by integer pixel and never filtered —
/// `ssr.slang`'s binding and its reasons, unchanged.
[[vk::binding(1, 0)]]
DepthTexture2D scene_depth D3D12_REGISTER(t1, space0);

/// The `Rgba16Float` frame this pass composites over: the forward pass's
/// output, or the reflection composite's where that ran.
[[vk::binding(2, 0)]]
Texture2D<float4> scene_color D3D12_REGISTER(t2, space0);

/// The column, after `integrateMain`: `rgb` is the radiance accumulated in
/// front of that froxel's slice, `a` the transmittance through all of it.
///
/// Read-only here, which is what lets the graph barrier the compute pass's
/// writes against this pass's reads rather than leaving them in one state.
[[vk::binding(3, 0)]]
StructuredBuffer<float4> volumetrics D3D12_REGISTER(t3, space0);

/// What each froxel's lights put into it, as `scatterMain` measured them —
/// what the punctual lights in its list scatter in `rgb`, what fraction of
/// the sun it could see in `w` — untouched by the scan, which is over
/// `volumetrics` alone.
///
/// The partial slice below is a piece of one froxel, so it owes that froxel's
/// scattering source; reading the numbers back is what keeps the cascade walk
/// and the light loop in one shader instead of running them again for every
/// pixel on screen.
[[vk::binding(4, 0)]]
StructuredBuffer<float4> lighting D3D12_REGISTER(t4, space0);

/// `crcbl_shaders::atmosphere::AerialView::rows` — the air in front of the
/// scene for this frame's sun, one `float4` of in-scatter and mean
/// transmittance per checkpoint, row slowest and slice fastest.
///
/// **A storage buffer with the trilinear read spelled out**, for the reason
/// `sky.slang`'s `sky_view` is one: a hardware filter's weights are
/// fixed-function arithmetic four rasterisers compute independently, and the
/// goldens are compared across all four. Bound on every frame and read only
/// where `aerial_params.w` says so; `crcbl_render::sky_pass` owns the ring.
///
/// **Appended past `lighting`, never inserted**, on `crcbl_render::sky_pass`'s
/// Metal ordering rule: `crcbl-mtl` numbers a buffer by counting the layout's
/// buffers in order and Slang numbers them by declaration order, so the two
/// agree only while both ascend.
[[vk::binding(5, 0)]]
StructuredBuffer<float4> aerial D3D12_REGISTER(t5, space0);

/// `crcbl_shaders::atmosphere::AERIAL_VIEW_WIDTH`, which is the azimuth away
/// from the sun.
static const uint AERIAL_VIEW_WIDTH = 32;

/// `crcbl_shaders::atmosphere::AERIAL_VIEW_HEIGHT`, which is the direction's
/// `y`.
static const uint AERIAL_VIEW_HEIGHT = 32;

/// `crcbl_shaders::atmosphere::AERIAL_VIEW_SLICES`, which is the distance.
static const uint AERIAL_VIEW_SLICES = 32;

/// `crcbl_shaders::atmosphere::AERIAL_MAX_DISTANCE`: how far the last
/// checkpoint is from the eye, in world units.
static const float AERIAL_MAX_DISTANCE = 1000.0;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

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
/// `pow`, and this result reaches a colour. See topic 44's shading rule.
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
/// `lit.w` scales the sun term alone, because the environment term is what
/// reaches the medium from every direction and an occluder in front of the sun
/// does not remove it. It arrives from `lighting` rather than from a cascade
/// walk of this file's own — `volumetric.slang` has the only copy — and so
/// does `lit.rgb`, the punctual lights' share, which that file's
/// `volumetric_punctual` summed through the phase function once per froxel.
float3 volumetric_source(float3 view_direction, float4 lit)
{
    float cos_theta = dot(params.sun_direction.xyz, view_direction);
    float3 sun = params.sun_radiance.rgb
               * volumetric_phase(params.sun_direction.w, cos_theta) * lit.w;
    return params.fog_color.rgb + sun + lit.rgb;
}

/// The aerial LUT's two direction axes blended bilinearly at one checkpoint.
///
/// `crcbl_shaders::atmosphere::AerialView`'s `bilinear`, with the taps already
/// found — `aerial_at` finds them once and reads two slices with them.
float4 aerial_slice(uint x0, uint x1, float fx, uint y0, uint y1, float fy, uint slice)
{
    uint row0 = y0 * AERIAL_VIEW_WIDTH;
    uint row1 = y1 * AERIAL_VIEW_WIDTH;
    float4 top = aerial[(row0 + x0) * AERIAL_VIEW_SLICES + slice] * (1.0 - fx)
               + aerial[(row0 + x1) * AERIAL_VIEW_SLICES + slice] * fx;
    float4 bottom = aerial[(row1 + x0) * AERIAL_VIEW_SLICES + slice] * (1.0 - fx)
                  + aerial[(row1 + x1) * AERIAL_VIEW_SLICES + slice] * fx;
    return top * (1.0 - fy) + bottom * fy;
}

/// `crcbl_shaders::atmosphere::AerialView::sample`: the LUT read trilinearly at
/// a direction's `y`, an azimuth cosine and a distance in world units.
///
/// The two direction axes are `sky.slang`'s `sky_view_at`'s two maps, with the
/// texels on the ends of each axis rather than half a texel in —
/// `crcbl_shaders::atmosphere`'s `aerial_axis_value` says why — so the scale is
/// the texel count less one and there is no half-texel offset. The distance
/// axis puts checkpoint `k` at `k + 1` slices out and the eye at
/// zero — no in-scatter, full transmittance — so a surface inside the first
/// slice blends from the identity, and one at `AERIAL_MAX_DISTANCE` or past it
/// takes the last checkpoint exactly. Held to the host's spelling by
/// `crcbl_shaders::atmosphere`'s
/// `the_composite_reads_the_aerial_lut_the_way_the_host_does`.
float4 aerial_at(float up, float azimuth_cosine, float distance)
{
    float u = sqrt(max(0.0, (1.0 - clamp(azimuth_cosine, -1.0, 1.0)) * 0.5));
    float clamped = clamp(up, -1.0, 1.0);
    float root = sqrt(abs(clamped));
    float v = 0.5 + 0.5 * (clamped >= 0.0 ? root : -root);

    float across = clamp(u, 0.0, 1.0) * float(AERIAL_VIEW_WIDTH - 1);
    float x0 = clamp(floor(across), 0.0, float(AERIAL_VIEW_WIDTH - 1));
    float x1 = min(x0 + 1.0, float(AERIAL_VIEW_WIDTH - 1));
    float fx = clamp(across - x0, 0.0, 1.0);

    float down = clamp(v, 0.0, 1.0) * float(AERIAL_VIEW_HEIGHT - 1);
    float y0 = clamp(floor(down), 0.0, float(AERIAL_VIEW_HEIGHT - 1));
    float y1 = min(y0 + 1.0, float(AERIAL_VIEW_HEIGHT - 1));
    float fy = clamp(down - y0, 0.0, 1.0);

    float depth = clamp(distance / AERIAL_MAX_DISTANCE, 0.0, 1.0) * float(AERIAL_VIEW_SLICES);
    float slice = min(floor(depth), float(AERIAL_VIEW_SLICES - 1));
    float fz = clamp(depth - slice, 0.0, 1.0);

    // The checkpoint before this slice's, which is the eye itself for the
    // first slice: the identity, and not a texel.
    float4 closer = float4(0.0, 0.0, 0.0, 1.0);
    if (slice > 0.0)
    {
        closer = aerial_slice(uint(x0), uint(x1), fx, uint(y0), uint(y1), fy, uint(slice) - 1u);
    }
    float4 further = aerial_slice(uint(x0), uint(x1), fx, uint(y0), uint(y1), fy, uint(slice));
    return closer * (1.0 - fz) + further * fz;
}

/// `crcbl_shaders::atmosphere::AerialView::along`: the air along a world
/// direction out to `distance`, by turning the direction into the LUT's
/// sun-relative coordinates exactly as `sky.slang`'s `atmosphere_radiance`
/// does for the sky.
float4 aerial_along(float3 direction, float distance)
{
    float3 sun = params.aerial_params.xyz;
    float view_flat = sqrt(direction.x * direction.x + direction.z * direction.z);
    float sun_flat = sqrt(sun.x * sun.x + sun.z * sun.z);
    float cosine = 1.0;
    if (view_flat > 0.0 && sun_flat > 0.0)
    {
        cosine = (direction.x * sun.x + direction.z * sun.z) / (view_flat * sun_flat);
    }
    return aerial_at(direction.y, cosine, distance);
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
    int2 pixel = int2(input.position.xy);
    float4 scene = scene_color.Load(int3(pixel, 0));

    uint grid_x = max(params.grid_x, 1u);
    uint grid_y = max(params.grid_y, 1u);
    uint slices = max(params.slices, 1u);
    uint tiles = grid_x * grid_y;
    uint tile_pixels = max(params.tile_pixels, 1u);

    float2 viewport = float2(float(max(params.viewport_x, 1u)), float(max(params.viewport_y, 1u)));
    float2 ndc = float2(
        (float(pixel.x) + 0.5) / viewport.x * 2.0 - 1.0,
        1.0 - (float(pixel.y) + 0.5) / viewport.y * 2.0);

    // **A pixel with nothing drawn on it is behind the whole column.** The
    // reversed-Z clear is the far plane, whose unprojection is at infinity for
    // a perspective camera — a division by zero rather than a large number — so
    // the end of the grid is taken directly instead.
    float depth = scene_depth.Load(int3(pixel, 0)).r;
    float view_depth = CLUSTER_FAR;
    if (depth > DEPTH_FAR)
    {
        float3 world = volumetric_unproject(ndc, depth);
        view_depth = dot(params.depth_row, float4(world, 1.0));
    }
    view_depth = clamp(view_depth, 0.0, CLUSTER_FAR);

    // The slice this pixel is in, and where that slice starts — found by
    // running `volumetric.slang`'s own multiply chain forward, so the boundary
    // the column was cut at and the boundary measured from here are the same
    // number rather than two roundings that agree. Reaching slice `k` takes `k`
    // multiplies from `CLUSTER_NEAR` on both sides, which is what makes them
    // the same number rather than merely equal ones.
    //
    // **Slice one begins at `CLUSTER_NEAR * CLUSTER_SLICE_RATIO`, not at
    // `CLUSTER_NEAR`.** Slice zero runs from the eye to the *end* of the grid's
    // first cell, because `scatterMain` extends it down to zero rather than
    // leaving the near field out of the column. Starting this walk one boundary
    // early charges every pixel the prefix of the slice in front of its own —
    // which draws a plausibly foggy frame that disagrees with the closed form
    // by a whole cell at the near end.
    uint slice = 0;
    float slice_start = 0.0;
    float next_start = CLUSTER_NEAR * CLUSTER_SLICE_RATIO;
    while (slice + 1 < slices && next_start <= view_depth)
    {
        slice_start = next_start;
        next_start *= CLUSTER_SLICE_RATIO;
        slice += 1;
    }

    // The tile the pixel is in, clamped for `mesh.slang`'s `froxel_of` reason:
    // the grid is `ceil(extent / tile)` wide, so the last column and row are
    // part-empty and a pixel in the part that is not there would index the next
    // row's froxel.
    uint tile_x = min(uint(max(pixel.x, 0)) / tile_pixels, grid_x - 1);
    uint tile_y = min(uint(max(pixel.y, 0)) / tile_pixels, grid_y - 1);
    uint froxel = tile_x + tile_y * grid_x + slice * tiles;

    // **The air first**: the surface with the atmosphere's air in front of it,
    // and the scene colour itself wherever the air is not composed — see the
    // header. Only where the depth is not the clear value, which is what keeps
    // the air off the sky: `sky.slang` has already integrated the whole of a
    // ray that leaves the scene.
    float3 surface = scene.rgb;
    if (params.aerial_params.w > 0.0 && depth > DEPTH_FAR)
    {
        float3 offset = volumetric_unproject(ndc, depth) - params.eye.xyz;
        float distance = length(offset);
        float3 direction = distance > 0.0 ? offset / distance : float3(0.0, 1.0, 0.0);
        float4 air = aerial_along(direction, distance);
        surface = surface * air.a + air.rgb;
    }

    // A frame whose scatter and scan did not run — an atmosphere with the fog
    // effect off — has no column to read, and `volumetrics` holds whatever the
    // last frame that ran them left there.
    if (params.fog_params.w > 0.0 || froxel >= params.froxel_count)
    {
        return float4(surface, scene.a);
    }
    float4 prefix = volumetrics[froxel];

    // The part of this pixel's own slice that is in front of it, along **this
    // pixel's** ray rather than its tile's centre ray. The prefix is the tile's,
    // which is the froxel quantisation the header names; the near field, where
    // that quantisation would be most visible, is almost entirely this term.
    float3 near_point = volumetric_unproject(ndc, 1.0);
    float near_depth = max(dot(params.depth_row, float4(near_point, 1.0)), 1e-6);
    float3 along = (near_point - params.eye.xyz) / near_depth;
    float3 from = params.eye.xyz + along * slice_start;
    float3 to = params.eye.xyz + along * max(view_depth, slice_start);

    float reference = params.fog_params.z;
    float3 segment = to - from;
    float length_of = length(segment);
    float tau = fog_optical_depth(params.fog_params.x, params.fog_params.y, from.y - reference,
                                  to.y - reference, length_of);
    float partial_survives = fog_exp_neg(tau);
    // The same source `volumetric.slang` gives a whole slice, along this
    // pixel's own ray — which is the direction it is already looking, so the
    // phase function is evaluated per pixel here and per tile there. A
    // degenerate segment scatters nothing, so its fallback axis is
    // unobservable.
    float3 view_direction = length_of > 1e-6 ? segment / length_of : float3(0.0, 0.0, 1.0);
    // The froxel's own lighting, not a second lookup: this segment is inside
    // the slice `scatterMain` measured, so charging it a different fraction of
    // the sun or a different glow than the froxel around it would put a seam
    // at every slice boundary — the one place the composite is meant to be
    // exact.
    float3 partial_radiance =
        volumetric_source(view_direction, lighting[froxel]) * (1.0 - partial_survives);

    float3 lit = surface * (prefix.a * partial_survives) + prefix.rgb
               + prefix.a * partial_radiance;
    return float4(lit, scene.a);
}
