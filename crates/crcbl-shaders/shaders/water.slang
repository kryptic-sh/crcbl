// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/55-water.md` rung 1: the surface of a still body of water, drawn
// after the opaque frame and composited over it by this pass alone.
//
// # What the pass reads and writes
//
// The vertex stage pulls `crcbl_water::surface_mesh`'s grid out of
// `water_vertices`, and the pipeline depth-tests it against the opaque frame's
// depth with writes on — so a fragment exists only where the surface is nearer
// than the geometry already drawn, and afterwards the depth buffer holds the
// surface for everything that tests against it. The colour target is the HDR
// frame itself, loaded; this fragment writes the whole composite, so there is
// no blend state. `water_copy.slang` put the frame's colour and depth into
// `scene_color` and `scene_depth` below first, because a pass cannot sample the
// image it is attached to.
//
// # The composite
//
// ```text
// straight   = scene_color at this pixel             the unmodified opaque frame
// behind     = scene_color where the refracted ray lands, unless that is in front
// thickness  = |surface - the opaque point behind it|
// T          = e^(-(absorption + scattering) * thickness), per channel
// scattered  = albedo * irradiance * (1 - T)
// F          = Schlick at WATER_FRESNEL_F0, an integer fifth power
// water      = behind * T * (1 - F) + fog(F * environment + (1 - F) * scattered)
// out        = straight * (1 - fade) + water * fade,  fade = thickness / WATER_SHORE_FADE
// ```
//
// **No transcendental reaches a colour** (`docs/plan/44-lighting.md`): the
// exponential is `fog_exp_neg`, Snell's law is a square root, and Schlick's
// power is four multiplies.
//
// # What is copied, and what holds each copy
//
// This repo has no `#include` by design — the manifest hashes one source per
// artifact — so every function below that another shader also carries is that
// shader's body, copied, and a guard in `crcbl_shaders` compares the two:
//
// * the three uniform blocks this pass binds — `FrameUniforms` from `mesh.slang`,
//   `SsrParams` from `ssr.slang`, `VolumetricParams` from
//   `volumetric_composite.slang` — so each is the very buffer that pass is
//   handed, read at the offsets it was written at;
// * the sun's cascade walk, receiver biases and all, from `mesh.slang`;
// * the sky and probe environment a reflection falls back to, from `ssr.slang`;
// * the fog arithmetic, and the froxel lookup of `volumetric_composite.slang`.
//
// `crcbl_shaders::water`'s tests are where each is compared, and
// `crcbl_shaders::fog`, `crcbl_shaders::volumetric`, `crcbl_shaders::probe` and
// `crcbl_shaders::probe_visibility` list this file beside the others they hold.

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

static const uint SHADOW_CASCADES = 2;
static const uint SHADOW_LIGHT_TILES = 14;
static const uint SHADOW_ATLAS_TILES = 16;
static const uint PROBE_LEVELS = 4;

/// `mesh.slang`'s frame block, bound here as the very buffer the forward pass
/// read: the camera, the ambient, the sun's cascades and the analytic fog.
struct FrameUniforms
{
    float4x4 view_proj;
    float4 camera_position;
    float4 ambient;
    float4x4 shadow_view_proj[SHADOW_CASCADES];
    float4 cascade_far;
    float4 shadow_params;
    uint4 cluster_grid;
    float4x4 light_view_proj[SHADOW_LIGHT_TILES];
    uint4 probe_counts;
    uint4 probe_levels;
    float4 probe_level_origin[PROBE_LEVELS];
    float4 probe_level_inv_spacing[PROBE_LEVELS];
    uint4 probe_level_offset[PROBE_LEVELS];
    float4 lod_params;
    float4 fog_params;
    float4 fog_color;
    float4 sky_sh_r;
    float4 sky_sh_g;
    float4 sky_sh_b;
    float4x4 previous_view_proj;
    uint4 vertex_pool;
    float4 shadow_atlas_rect[SHADOW_ATLAS_TILES];
    uint4 shadow_filter;
};

/// `ssr.slang`'s probe row.
struct GpuProbe
{
    float4 sh_r;
    float4 sh_g;
    float4 sh_b;
};

/// `ssr.slang`'s block, bound here as the very buffer the reflection pass read:
/// the probe volume's header and the sky a reflection falls back to.
struct SsrParams
{
    float4x4 inv_proj;
    float4x4 proj;
    float4x4 inv_view;
    uint4 probe_counts;
    uint4 probe_levels;
    float4 probe_level_origin[PROBE_LEVELS];
    float4 probe_level_inv_spacing[PROBE_LEVELS];
    uint4 probe_level_offset[PROBE_LEVELS];
    uint4 hiz;
    float4 sky[3];
    float4 atmosphere;
};

/// `volumetric_composite.slang`'s block, bound here as the very buffer the
/// froxel composite read: the inverse view-projection, the froxel grid and the
/// medium the column was integrated through.
struct VolumetricParams
{
    float4x4 inverse_view_proj;
    float4 eye;
    float4 depth_row;
    float4 fog_params;
    float4 fog_color;
    float4 sun_direction;
    float4 sun_radiance;
    float4x4 shadow_view_proj[SHADOW_CASCADES];
    float4 cascade_far;
    float4 shadow_params;
    uint grid_x;
    uint grid_y;
    uint slices;
    uint tile_pixels;
    uint viewport_x;
    uint viewport_y;
    uint froxel_count;
    uint pad0;
    float4x4 light_view_proj[SHADOW_LIGHT_TILES];
    float4 shadow_atlas_rect[SHADOW_ATLAS_TILES];
};

/// What this pass needs that no other block carries, matching
/// `crcbl_shaders::water::WaterParams`.
struct WaterParams
{
    /// The unit direction **towards** the sun in `xyz`; `w` unread.
    float4 sun_direction;
    /// The sun's colour, the radiance a surface facing it receives, in `rgb`;
    /// `w` unread.
    float4 sun_color;
    /// `x` is `1` on a frame whose froxel volume ran — so the air in front of
    /// the surface is read out of `volumetrics` rather than evaluated from
    /// `FrameUniforms::fog_params` — and `0` otherwise; `yzw` unread.
    uint4 froxels;
};

/// One vertex of `crcbl_water::surface_mesh`'s grid, matching
/// `crcbl_shaders::water::WaterVertex`: the world position in `xyz`, and the
/// body it belongs to in `body.x`.
struct WaterVertex
{
    float4 position;
    uint4 body;
};

/// One body's medium, matching `crcbl_shaders::water::WaterMedium`: absorption
/// and scattering per metre in `rgb`, `w` unread.
struct WaterMedium
{
    float4 absorption;
    float4 scattering;
};

[[vk::binding(0, 0)]]
ConstantBuffer<WaterParams> water D3D12_REGISTER(b0, space0);

/// Read by both stages: the vertex stage projects with `view_proj`.
[[vk::binding(1, 0)]]
ConstantBuffer<FrameUniforms> frame D3D12_REGISTER(b1, space0);

[[vk::binding(2, 0)]]
ConstantBuffer<SsrParams> camera D3D12_REGISTER(b2, space0);

[[vk::binding(3, 0)]]
ConstantBuffer<VolumetricParams> params D3D12_REGISTER(b3, space0);

/// Every body's surface grid, one buffer for all of them; each draw is a range
/// of the index buffer into it.
[[vk::binding(4, 0)]]
StructuredBuffer<WaterVertex> water_vertices D3D12_REGISTER(t4, space0);

/// One row per body, indexed by `WaterVertex::body`.
[[vk::binding(5, 0)]]
StructuredBuffer<WaterMedium> media D3D12_REGISTER(t5, space0);

/// `water_copy.slang`'s copy of the HDR frame as the opaque passes left it.
/// Read with `Load` only, on `ssr.slang`'s terms: a hardware filter is
/// fixed-function arithmetic four rasterisers compute independently.
[[vk::binding(6, 0)]]
Texture2D<float4> scene_color D3D12_REGISTER(t6, space0);

/// `water_copy.slang`'s copy of the opaque frame's depth, read by `Load`.
[[vk::binding(7, 0)]]
DepthTexture2D scene_depth D3D12_REGISTER(t7, space0);

/// The shadow atlas and its comparison sampler, as `mesh.slang` binds them.
[[vk::binding(8, 0)]]
DepthTexture2D shadow_atlas D3D12_REGISTER(t8, space0);

[[vk::binding(9, 0)]]
SamplerComparisonState shadow_sampler D3D12_REGISTER(s9, space0);

/// The irradiance probe rows, as `ssr.slang` binds them.
[[vk::binding(10, 0)]]
StructuredBuffer<GpuProbe> probes D3D12_REGISTER(t10, space0);

/// `crcbl_shaders::sky_prefilter`'s table, as `ssr.slang` binds it.
[[vk::binding(11, 0)]]
Texture2D<float4> sky_prefilter D3D12_REGISTER(t11, space0);

/// The per-probe visibility maps, as `ssr.slang` binds them.
[[vk::binding(12, 0)]]
Texture2DArray<float4> probe_visibility D3D12_REGISTER(t12, space0);

/// The sky-view LUT, as `ssr.slang` binds it — `crcbl_render::sky_pass`'s slot.
[[vk::binding(13, 0)]]
StructuredBuffer<float4> sky_view D3D12_REGISTER(t13, space0);

/// The froxel column and its lighting, as `volumetric_composite.slang` binds
/// them. Read only on a frame whose `WaterParams::froxels.x` says the volume ran.
[[vk::binding(14, 0)]]
StructuredBuffer<float4> volumetrics D3D12_REGISTER(t14, space0);

[[vk::binding(15, 0)]]
StructuredBuffer<float4> lighting D3D12_REGISTER(t15, space0);

static const float DEPTH_FAR = 0.0;
static const uint CLUSTER_DEPTH_SLICES = 24;
static const float CLUSTER_NEAR = 0.1;
static const float CLUSTER_FAR = 1000.0;
static const float CLUSTER_SLICE_RATIO = 1.4677993;
static const float FOG_LOG2_E = 1.44269504;
static const float FOG_LN2_HI = 0.69311523;
static const float FOG_LN2_LO = 3.1946183e-5;
static const float FOG_MAX_ARGUMENT = 87.0;
static const float FOG_MAX_OPTICAL_DEPTH = 32.0;
static const float FOG_SERIES_CUTOFF = 0.125;
static const float FOG_KERNEL[8] = {
    1.0, 1.0, 0.5, 0.16666667, 0.041666668, 0.008333334, 0.0013888889, 0.00019841270
};
static const float FOG_RATIO_KERNEL[5] = {
    1.0, 0.5, 0.16666667, 0.041666668, 0.008333334
};
static const float VOLUMETRIC_INV_FOUR_PI = 0.079577470;
static const float VOLUMETRIC_MAX_ANISOTROPY = 0.99;
static const uint SKY_VIEW_WIDTH = 96;
static const uint SKY_VIEW_HEIGHT = 64;
static const float PROBE_TRANSFER_L0 = 3.14159265358979323846;
static const float PROBE_TRANSFER_L1 = 2.0943951023931954923;
static const float PROBE_LEVEL_BAND = 0.25;
static const float PROBE_VISIBILITY_SIDE = 16.0;
static const float PROBE_VISIBILITY_BORDER = 1.0;
static const float PROBE_SURFACE_BIAS = 0.05;
static const float PROBE_OCCLUDED_WEIGHT = 0.0001;
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
static const float SHADOW_CASTER_REACH = 40.0;
static const float SHADOW_SUN_TAN_RADIUS = 0.02;
static const float SHADOW_SEARCH_TEXELS = 8.0;
static const uint SHADOW_SEARCH_TAPS = 16;
static const float2 SHADOW_SEARCH_DISC[SHADOW_SEARCH_TAPS] = {
    float2(+0.176777, +0.000000), float2(-0.225772, +0.206826),
    float2(+0.034558, -0.393771), float2(+0.284571, +0.371173),
    float2(-0.522223, -0.092374), float2(+0.494695, -0.314685),
    float2(-0.165466, +0.615525), float2(-0.315561, -0.607594),
    float2(+0.684642, +0.250030), float2(-0.712256, +0.294009),
    float2(+0.343354, -0.733729), float2(+0.253730, +0.808932),
    float2(-0.764746, -0.443186), float2(+0.897134, -0.197232),
    float2(-0.547507, +0.778772), float2(-0.126487, -0.976090),
};
static const uint SHADOW_FILTER_PCSS = 0u;
static const uint SHADOW_FILTER_DISC = 1u;
static const uint SHADOW_FILTER_BOX = 2u;
static const float CASCADE_FADE_FRACTION = 0.1;
static const uint CASCADE_NONE = SHADOW_CASCADES;

/// Schlick's reflectance at normal incidence for an air–water interface,
/// `((1.333 - 1) / (1.333 + 1))²` rounded to the value every water model in
/// `docs/plan/55-water.md` uses. `crcbl_shaders::water::FRESNEL_F0`.
static const float WATER_FRESNEL_F0 = 0.02;

/// The ratio of refractive indices a ray entering water from air bends by,
/// `1 / 1.333`. `crcbl_shaders::water::ETA`.
static const float WATER_ETA = 0.75;

/// How much water, in metres of view ray, it takes for the surface to be drawn
/// whole. A pixel whose floor is nearer than this fades toward the opaque frame,
/// which is what keeps the edge of a pool from ending in a hard line where the
/// floor meets the level. `crcbl_shaders::water::SHORE_FADE`.
static const float WATER_SHORE_FADE = 0.25;

/// How far a ray under the surface travels when nothing opaque is behind it —
/// `CLUSTER_FAR`'s thousand metres, which every absorption worth naming has
/// taken to nothing long before. `crcbl_shaders::water::OPEN_THICKNESS`.
static const float WATER_OPEN_THICKNESS = 1000.0;

float fog_exp_neg(float x)
{
    float clamped = clamp(x, -FOG_MAX_ARGUMENT, FOG_MAX_ARGUMENT);

    // e^-x = 2^-n * e^-r, with |r| at most half of ln 2.
    float n = floor(clamped * FOG_LOG2_E + 0.5);
    float reduced = (clamped - n * FOG_LN2_HI) - n * FOG_LN2_LO;

    float u = -reduced;
    float kernel = FOG_KERNEL[7];
    for (int term = 6; term >= 0; --term)
    {
        kernel = kernel * u + FOG_KERNEL[term];
    }

    // 2^-n by exponent field. The clamp above is what keeps this inside the
    // normal range, so no bias arithmetic is needed for the subnormal end.
    return kernel * asfloat(uint(127 - int(n)) << 23);
}

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

float fog_transmittance(float optical_depth)
{
    return fog_exp_neg(max(optical_depth, 0.0));
}

float3 volumetric_unproject(float2 ndc, float depth)
{
    float4 world = mul(params.inverse_view_proj, float4(ndc, depth, 1.0));
    return world.xyz / world.w;
}

float volumetric_phase(float g, float cos_theta)
{
    float a = clamp(g, -VOLUMETRIC_MAX_ANISOTROPY, VOLUMETRIC_MAX_ANISOTROPY);
    float d = 1.0 + a * a - 2.0 * a * clamp(cos_theta, -1.0, 1.0);
    return VOLUMETRIC_INV_FOUR_PI * (1.0 - a * a) / (d * sqrt(d));
}

float3 volumetric_source(float3 view_direction, float4 lit)
{
    float cos_theta = dot(params.sun_direction.xyz, view_direction);
    float3 sun = params.sun_radiance.rgb
               * volumetric_phase(params.sun_direction.w, cos_theta) * lit.w;
    return params.fog_color.rgb + sun + lit.rgb;
}

float3 sky_irradiance(float3 normal)
{
    float4 basis = float4(normal, 1.0);
    return max(float3(dot(frame.sky_sh_r, basis),
                      dot(frame.sky_sh_g, basis),
                      dot(frame.sky_sh_b, basis)),
               float3(0.0, 0.0, 0.0));
}

float shadow_normal_offset(float3 geometric_normal, float3 to_light)
{
    float cosine = saturate(dot(geometric_normal, to_light));
    return sqrt(saturate(1.0 - cosine * cosine));
}

float2 shadow_rotation(float2 pixel)
{
    uint2 cell = uint2(pixel) & 3u;
    return SHADOW_ROTATIONS[SHADOW_DITHER[cell.y * 4u + cell.x]];
}

uint shadow_filter_mode(float2 pixel)
{
    return uint(pixel.x) < frame.shadow_filter.z ? frame.shadow_filter.x
                                                 : frame.shadow_filter.y;
}

float2 atlas_uv(float4 rect, float2 tile_uv)
{
    return rect.zw + tile_uv * rect.xy;
}

float4 atlas_rect(uint tile)
{
    return frame.shadow_atlas_rect[tile];
}

float2 atlas_step(float4 rect)
{
    return frame.shadow_params.xy / rect.xy;
}

float tile_texels(float4 rect)
{
    return rect.x / frame.shadow_params.x;
}

bool atlas_rect_is_empty(float4 rect)
{
    return !(rect.x > 0.0);
}

float tile_tap(float4 rect, float2 texel_step, float2 tile_uv, float2 spoke, float2 rotation,
               float reference)
{
    float2 tile_min = float2(0.5, 0.5) * texel_step;
    // The rotation, written out: a complex multiply by `(cos, sin)`.
    float2 turned = float2(spoke.x * rotation.x - spoke.y * rotation.y,
                           spoke.x * rotation.y + spoke.y * rotation.x);
    float2 tap = clamp(tile_uv + turned * texel_step, tile_min, 1.0 - tile_min);
    return shadow_atlas.SampleCmpLevelZero(shadow_sampler, atlas_uv(rect, tap), reference);
}

float tile_pcf(uint tile, float2 tile_uv, float reference, float2 pixel, float radius)
{
    float2 rotation = shadow_rotation(pixel);
    float4 rect = atlas_rect(tile);
    if (atlas_rect_is_empty(rect))
    {
        return 1.0;
    }
    float2 texel_step = atlas_step(rect);

    float probe = 0.0;
    for (uint spot = 0u; spot < SHADOW_PROBE_TAPS; ++spot)
    {
        probe += tile_tap(rect, texel_step, tile_uv,
                          SHADOW_DISC[SHADOW_PROBE_INDEX[spot]] * radius, rotation, reference);
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
        visibility += tile_tap(rect, texel_step, tile_uv, SHADOW_DISC[index] * radius, rotation,
                               reference);
    }
    return visibility / float(SHADOW_TAPS);
}

float tile_box_pcf(uint tile, float2 tile_uv, float reference)
{
    float4 rect = atlas_rect(tile);
    if (atlas_rect_is_empty(rect))
    {
        return 1.0;
    }
    float2 texel_step = atlas_step(rect);

    float visibility = 0.0;
    for (int y = -1; y <= 1; ++y)
    {
        for (int x = -1; x <= 1; ++x)
        {
            visibility += tile_tap(rect, texel_step, tile_uv, float2(float(x), float(y)),
                                   float2(1.0, 0.0), reference);
        }
    }
    return visibility / 9.0;
}

float sun_penumbra_texels(uint cascade, float2 tile_uv, float reference, float2 rotation)
{
    float2 texel = frame.shadow_params.xy;
    float4 rect = atlas_rect(cascade);
    float2 texel_step = atlas_step(rect);
    float2 tile_min = float2(0.5, 0.5) * texel_step;
    // The atlas's extent in texels, which is what turns a normalised atlas
    // coordinate into the integer `Load` wants.
    float2 extent = float2(1.0, 1.0) / texel;

    float sum = 0.0;
    float found = 0.0;
    for (uint index = 0u; index < SHADOW_SEARCH_TAPS; ++index)
    {
        float2 spoke = SHADOW_SEARCH_DISC[index] * SHADOW_SEARCH_TEXELS;
        float2 turned = float2(spoke.x * rotation.x - spoke.y * rotation.y,
                               spoke.x * rotation.y + spoke.y * rotation.x);
        float2 tap = clamp(tile_uv + turned * texel_step, tile_min, 1.0 - tile_min);
        // `min` against the last texel rather than trust to the clamp above: that
        // one is in tile space and half a texel inside, so the product below
        // lands inside the atlas by construction — but it is a float product,
        // and an out-of-range `Load` is undefined rather than clamped.
        int2 coord = int2(min(atlas_uv(rect, tap) * extent, extent - float2(1.0, 1.0)));
        float depth = shadow_atlas.Load(int3(coord, 0));
        if (depth > reference)
        {
            sum += depth;
            found += 1.0;
        }
    }

    if (found <= 0.0)
    {
        return SHADOW_FILTER_TEXELS;
    }

    // Shadow-clip depth back to metres. The cascade's box is `2 * radius` deep
    // through the sphere plus `SHADOW_CASTER_REACH` in front of it, and an
    // orthographic projection spreads that range linearly over `0..1`, so one
    // unit of `ndc.z` is the whole of it. `cascade_matrix` on the host is where
    // those two ends are set.
    float radius = frame.cascade_far[cascade];
    float separation = (sum / found - reference) * (2.0 * radius + SHADOW_CASTER_REACH);
    float texel_world = 2.0 * radius / tile_texels(rect);
    return clamp(separation * SHADOW_SUN_TAN_RADIUS / texel_world,
                 SHADOW_FILTER_TEXELS,
                 SHADOW_SEARCH_TEXELS);
}

float cascade_visibility(uint cascade, float3 world_position, float3 to_light,
                         float3 geometric_normal, float2 pixel)
{
    // The bias, and it is `punctual_visibility`'s exactly: two moves of the
    // world position before it is projected, each a multiple of the world
    // footprint of one texel of *this cascade's* map. A cascade is an
    // orthographic box of `2 * radius` fitted to `tile_texels` of its own
    // rectangle, and its radius is the split distance the caller's selection
    // read — the same arithmetic `crcbl_render::shadow::cascade_matrix` snaps
    // its origin to. The side comes out of the rectangle rather than out of a
    // constant because a map is no longer always a whole root cell; see
    // `tile_texels`.
    //
    // **A number in texels rather than in shadow-clip depth**, which is the
    // whole of this: acne is the map quantising a receiver's own depth away
    // across one texel, so the error to cover scales with the texel's footprint
    // and with the surface's slope. A constant added in clip instead scales with
    // the cascade's whole depth range — `2 * radius` plus `CASTER_REACH`, tens
    // of metres of it belonging to casters standing off the near plane — so the
    // same number meant 0.83 m of world slack on the outer cascade and lit a
    // strip along the foot of every wall.
    //
    // **The two moves are in different directions and cover different things.**
    // Along the normal is `shadow_normal_offset`'s, and it is what the slope
    // costs: it walks the lookup sideways across the map instead of raising the
    // depth it compares, so a grazing surface is read where it belongs and a
    // contact shadow keeps its contact. Along the light is the constant, and it
    // is the facet *seam* — two triangles of different slope biased by different
    // amounts, where the texel their shared edge falls in stores the steeper
    // one's depth and no offset read off either facet predicts the other's.
    //
    // What the two counts are and why they are what they are lives on the host,
    // in `crcbl_render::shadow`: `DEPTH_BIAS_TEXELS` and `NORMAL_OFFSET_TEXELS`
    // have the numbers and the frames they were read off.
    float4 rect = atlas_rect(cascade);
    if (atlas_rect_is_empty(rect))
    {
        // No map in this slot, so nothing here was ever rendered. **Before the
        // divide**, which is the whole point: see `atlas_rect_is_empty`.
        return 1.0;
    }
    float texel_world = 2.0 * frame.cascade_far[cascade] / tile_texels(rect);
    float3 biased = world_position
                  + geometric_normal
                        * (texel_world * frame.shadow_params.w
                           * shadow_normal_offset(geometric_normal, to_light))
                  + to_light * (texel_world * frame.shadow_params.z);

    float4 clip = mul(frame.shadow_view_proj[cascade], float4(biased, 1.0));
    // Orthographic, so `w` is 1 and the divide is a formality — written out
    // anyway because nothing in this file may assume which constructor built a
    // matrix, which is `Projection`'s whole point on the CPU side.
    float3 ndc = clip.xyz / clip.w;
    if (any(abs(ndc.xy) > 1.0) || ndc.z <= 0.0)
    {
        // Outside this cascade's box, or beyond its far plane. Lit: a shadow map
        // knows nothing about what it never rendered, and inventing a shadow
        // there is a black band across everything past the last split.
        return 1.0;
    }

    // Y-up NDC to top-left texture space, which is the flip `crcbl_render`'s
    // projections push into the viewport rather than into the matrix.
    float2 tile_uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);

    // Both moves are already in `ndc`, and they land in different components.
    // The one along `to_light` moved the reference *up* — reversed-Z, so a
    // larger depth is nearer the light, which is the direction that stops a
    // surface shadowing itself — and it cannot move `tile_uv`, because the
    // projection's own depth axis is that direction. The one along the normal is
    // the opposite: it is perpendicular to that axis in the amount
    // `shadow_normal_offset` returns, so what it moves is `tile_uv` and the
    // texel this fragment reads. That is the difference between raising a
    // comparison and taking a different one.
    // The filter's width, which for the sun is not a constant: a contact is
    // sharp and a shadow thrown from height is soft, and `sun_penumbra_texels`
    // is what reads the difference out of the map. The rotation is taken once
    // and handed to both so the disc the search walks is the disc the filter
    // draws.
    uint mode = shadow_filter_mode(pixel);
    if (mode == SHADOW_FILTER_BOX)
    {
        return tile_box_pcf(cascade, tile_uv, ndc.z);
    }
    if (mode == SHADOW_FILTER_DISC)
    {
        // The disc at its fixed reach: the ninth decision without the tenth, so
        // the penumbra is the same width wherever its caster stands.
        return tile_pcf(cascade, tile_uv, ndc.z, pixel, SHADOW_FILTER_TEXELS);
    }
    float2 rotation = shadow_rotation(pixel);
    float radius = sun_penumbra_texels(cascade, tile_uv, ndc.z, rotation);
    return tile_pcf(cascade, tile_uv, ndc.z, pixel, radius);
}

float sun_visibility(float3 world_position, float3 to_light, float n_dot_l,
                     float3 geometric_normal, float2 pixel, out uint selected,
                     out float fade)
{
    // **The two out-parameters are what the cascade view draws, and they are
    // written on every path out of here.** The alternative was for
    // `fragmentMain` to run this selection a second time beside the shading,
    // which is exactly the arrangement an overlay must not have: an overlay
    // derived independently can disagree with the lighting it claims to explain,
    // and the disagreement is invisible — a plausible tint over a plausible
    // shadow. Reported rather than recomputed, the tint *is* the selection.
    selected = CASCADE_NONE;
    fade = 0.0;
    if (n_dot_l <= 0.0)
    {
        return 1.0;
    }

    // The first cascade whose reach covers this fragment. Distance to the eye
    // rather than view-space z: the splits are radii around the camera, so a
    // fragment at the edge of the screen and one straight ahead at the same
    // distance land in the same cascade and the seam is a sphere rather than a
    // plane the camera can rotate.
    float eye_distance = length(world_position - frame.camera_position.xyz);
    uint cascade = SHADOW_CASCADES - 1;
    // Whether a cascade's reach actually covers this fragment, as against the
    // fallback above. The two are the same *index* and a different fact: past
    // the last split the shading still asks the last cascade — and gets "lit"
    // out of its box test — while the overlay must say "no cascade covers this",
    // because a tint there would colour the whole distance as if it were mapped.
    bool covered = false;
    for (uint index = 0; index < SHADOW_CASCADES; ++index)
    {
        if (eye_distance < frame.cascade_far[index])
        {
            cascade = index;
            covered = true;
            break;
        }
    }
    if (covered)
    {
        selected = cascade;
    }

    float visibility =
        cascade_visibility(cascade, world_position, to_light, geometric_normal, pixel);
    if (cascade + 1 >= SHADOW_CASCADES)
    {
        // The last cascade has nothing to fade into. Everything past its reach
        // selects it too — the loop's fallback — and gets its answer unmixed,
        // which the box test inside turns into "lit" soon enough.
        return visibility;
    }

    // The band, at the outer edge of this cascade's reach. `reach` is strictly
    // positive here: the loop only leaves a cascade selected whose far distance
    // *exceeds* this fragment's, and the one case where it does not — nothing
    // matched — returned above.
    float reach = frame.cascade_far[cascade];
    float band = reach * CASCADE_FADE_FRACTION;
    float blend = saturate((eye_distance - (reach - band)) / band);
    // Before the early return, not after it: `saturate` has already made a
    // fragment short of the band an exact zero, and reporting it there as well
    // is what keeps this parameter written on every path.
    fade = blend;
    if (blend <= 0.0)
    {
        return visibility;
    }

    // The next cascade is guaranteed to contain this fragment, which is what
    // makes the mix well defined rather than a blend with the `1.0` an outside
    // fragment gets: the cascades are spheres about the same eye with strictly
    // growing radii — `crcbl_render::shadow::cascade_matrix` fits every one of
    // them to `eye` — so anything inside cascade `i` is inside cascade `i + 1`
    // by construction, not by a margin someone has to maintain.
    float next =
        cascade_visibility(cascade + 1, world_position, to_light, geometric_normal, pixel);
    return lerp(visibility, next, blend);
}

float2 decode_fixed_pair(float4 texel)
{
    return float2(texel.r * 65280.0 + texel.g * 255.0, texel.b * 65280.0 + texel.a * 255.0)
         / 65535.0;
}

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

float2 sky_prefilter_at(float up, float roughness)
{
    return fixed_pair_at(sky_prefilter, float2(up, roughness));
}

float3 sky_prefiltered(float3 direction, float roughness)
{
    float up = clamp(direction.y, -1.0, 1.0);
    float2 weights = sky_prefilter_at(abs(up), roughness);
    float3 far = up >= 0.0 ? camera.sky[0].rgb : camera.sky[2].rgb;
    float3 opposite = up >= 0.0 ? camera.sky[2].rgb : camera.sky[0].rgb;
    float horizon = 1.0 - weights.x - weights.y;
    return camera.sky[1].rgb * horizon + far * weights.x + opposite * weights.y;
}

float3 sky_view_at(float up, float azimuth_cosine)
{
    float u = sqrt(max(0.0, (1.0 - clamp(azimuth_cosine, -1.0, 1.0)) * 0.5));
    float clamped = clamp(up, -1.0, 1.0);
    float root = sqrt(abs(clamped));
    float v = 0.5 + 0.5 * (clamped >= 0.0 ? root : -root);

    float across = clamp(u, 0.0, 1.0) * float(SKY_VIEW_WIDTH) - 0.5;
    float x0 = clamp(floor(across), 0.0, float(SKY_VIEW_WIDTH - 1));
    float x1 = min(x0 + 1.0, float(SKY_VIEW_WIDTH - 1));
    float fx = clamp(across - x0, 0.0, 1.0);

    float down = clamp(v, 0.0, 1.0) * float(SKY_VIEW_HEIGHT) - 0.5;
    float y0 = clamp(floor(down), 0.0, float(SKY_VIEW_HEIGHT - 1));
    float y1 = min(y0 + 1.0, float(SKY_VIEW_HEIGHT - 1));
    float fy = clamp(down - y0, 0.0, 1.0);

    uint row0 = uint(y0) * SKY_VIEW_WIDTH;
    uint row1 = uint(y1) * SKY_VIEW_WIDTH;
    float3 top = sky_view[row0 + uint(x0)].rgb * (1.0 - fx)
               + sky_view[row0 + uint(x1)].rgb * fx;
    float3 bottom = sky_view[row1 + uint(x0)].rgb * (1.0 - fx)
                  + sky_view[row1 + uint(x1)].rgb * fx;
    return top * (1.0 - fy) + bottom * fy;
}

float3 atmosphere_radiance(float3 direction)
{
    float3 sun = camera.atmosphere.xyz;
    float view_flat = sqrt(direction.x * direction.x + direction.z * direction.z);
    float sun_flat = sqrt(sun.x * sun.x + sun.z * sun.z);
    float cosine = 1.0;
    if (view_flat > 0.0 && sun_flat > 0.0)
    {
        cosine = (direction.x * sun.x + direction.z * sun.z) / (view_flat * sun_flat);
    }
    return sky_view_at(direction.y, cosine);
}

float3 sky_environment(float3 direction, float roughness, float share)
{
    float3 bands = sky_prefiltered(direction, roughness);
    if (camera.atmosphere.w <= 0.0)
    {
        return bands;
    }
    // The two-ended form and not `lerp`, because `crcbl_shaders::atmosphere`'s
    // host mirror of this line is written the same way and the two do not agree
    // in floating point.
    return bands * (1.0 - share) + atmosphere_radiance(direction) * share;
}

float sign_not_zero(float value)
{
    return value >= 0.0 ? 1.0 : -1.0;
}

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

float probe_weight(uint index, float3 probe_position, float3 world_position, float3 normal)
{
    return max(probe_chebyshev(index, probe_position, world_position, normal),
               PROBE_OCCLUDED_WEIGHT);
}

uint probe_wrap(uint cell, uint offset, uint count)
{
    uint at = cell + offset;
    return at >= count ? at - count : at;
}

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

struct WeightedProbe
{
    GpuProbe sh;
    float weight;
};

WeightedProbe lerp_probe(WeightedProbe a, WeightedProbe b, float t)
{
    WeightedProbe blended;
    blended.sh.sh_r = lerp(a.sh.sh_r, b.sh.sh_r, t);
    blended.sh.sh_g = lerp(a.sh.sh_g, b.sh.sh_g, t);
    blended.sh.sh_b = lerp(a.sh.sh_b, b.sh.sh_b, t);
    blended.weight = lerp(a.weight, b.weight, t);
    return blended;
}

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

/// What the air between the eye and a point does to light leaving that point:
/// the share that survives, and the radiance the air adds in front of it.
struct WaterAir
{
    float survives;
    float3 inscatter;
};

/// The air in front of the surface at `pixel`, whose depth is `depth`, read out
/// of the froxel column exactly as `volumetric_composite.slang` reads it for the
/// opaque frame: the column's exclusive prefix in front of the pixel's slice,
/// and the partial slice integrated along this pixel's own ray.
///
/// The composite writes `scene * (prefix.a * partial) + prefix.rgb + prefix.a *
/// partial_radiance`; this returns that composite's two halves, so a caller
/// applies it to a radiance of its own.
WaterAir water_froxel_air(int2 pixel, float depth)
{
    WaterAir air;
    air.survives = 1.0;
    air.inscatter = float3(0.0, 0.0, 0.0);

    uint grid_x = max(params.grid_x, 1u);
    uint grid_y = max(params.grid_y, 1u);
    uint slices = max(params.slices, 1u);
    uint tiles = grid_x * grid_y;
    uint tile_pixels = max(params.tile_pixels, 1u);
    float2 viewport = float2(float(max(params.viewport_x, 1u)), float(max(params.viewport_y, 1u)));
    float2 ndc = float2(
        (float(pixel.x) + 0.5) / viewport.x * 2.0 - 1.0,
        1.0 - (float(pixel.y) + 0.5) / viewport.y * 2.0);
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
    if (froxel >= params.froxel_count)
    {
        return air;
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

    air.survives = prefix.a * partial_survives;
    air.inscatter = prefix.rgb + prefix.a * partial_radiance;
    return air;
}

/// The air in front of the surface point `surface`, evaluated from the frame
/// block's exponential height fog exactly as `mesh.slang` fogs a fragment.
WaterAir water_analytic_air(float3 surface)
{
    float3 fog_offset = frame.camera_position.xyz - surface;
    float fog_depth = fog_optical_depth(frame.fog_params.x, frame.fog_params.y,
                                        frame.camera_position.y - frame.fog_params.z,
                                        surface.y - frame.fog_params.z,
                                        length(fog_offset));
    float fog_survives = fog_transmittance(fog_depth);

    WaterAir air;
    air.survives = fog_survives;
    air.inscatter = frame.fog_color.rgb * (1.0 - fog_survives);
    return air;
}

/// The direction a unit ray `incident` continues in after entering a surface
/// whose unit normal `normal` faces it, bending by `eta` — Snell's law as a
/// square root, with no trigonometry. `k` below is `cos²` of the refracted
/// angle, and is never negative for a ray entering the denser medium, so there
/// is no total internal reflection to handle from above.
float3 water_refract(float3 incident, float3 normal, float eta)
{
    float cos_incident = -dot(incident, normal);
    float k = 1.0 - eta * eta * (1.0 - cos_incident * cos_incident);
    return incident * eta + normal * (eta * cos_incident - sqrt(max(k, 0.0)));
}

/// The pixel centre of `pixel` as normalised device coordinates, `y` up, in a
/// frame `extent` texels across — the addressing
/// `volumetric_composite.slang` unprojects a pixel with.
float2 water_ndc(int2 pixel, float2 extent)
{
    return float2((float(pixel.x) + 0.5) / extent.x * 2.0 - 1.0,
                  1.0 - (float(pixel.y) + 0.5) / extent.y * 2.0);
}

/// The world point the opaque frame holds at `pixel`, or — at the far plane,
/// where there is no surface to unproject — the point `WATER_OPEN_THICKNESS`
/// beyond `surface` along `direction`.
float3 water_opaque_point(int2 pixel, float depth, float2 extent, float3 surface,
                          float3 direction)
{
    if (depth <= DEPTH_FAR)
    {
        return surface + direction * WATER_OPEN_THICKNESS;
    }
    return volumetric_unproject(water_ndc(pixel, extent), depth);
}

struct SurfaceOutput
{
    float4 position : SV_Position;
    float3 world_position : TEXCOORD0;
    /// The body this fragment belongs to. `nointerpolation` because it is a
    /// row index, the same at every corner of a triangle.
    nointerpolation uint body : TEXCOORD1;
};

[shader("vertex")]
SurfaceOutput vertexMain(uint index : SV_VertexID)
{
    // The index buffer's values are absolute and the draw's base vertex is
    // zero, so `SV_VertexID` is the row on every target — `ui.slang`'s
    // arrangement.
    WaterVertex vertex = water_vertices[index];
    SurfaceOutput output;
    output.world_position = vertex.position.xyz;
    output.position = mul(frame.view_proj, float4(vertex.position.xyz, 1.0));
    output.body = vertex.body.x;
    return output;
}

[shader("fragment")]
float4 fragmentMain(SurfaceOutput input) : SV_Target
{
    uint width;
    uint height;
    scene_depth.GetDimensions(width, height);
    float2 extent = float2(float(width), float(height));
    int2 last = int2(int(width) - 1, int(height) - 1);
    int2 pixel = int2(input.position.xy);
    float surface_depth = input.position.z;
    float3 surface = input.world_position;
    WaterMedium medium = media[input.body];
    // Rung 1's surface is flat, so its normal is straight up everywhere.
    float3 up = float3(0.0, 1.0, 0.0);

    // Eye → surface, and how steeply it meets the plane.
    float3 incident = normalize(surface - frame.camera_position.xyz);
    float cos_incident = saturate(-incident.y);

    // --- what is behind the surface ---
    //
    // The straight-through pixel first: it is the unmodified opaque frame the
    // shoreline fades toward, and its depth below the surface is how far the
    // refracted ray is walked.
    float4 straight = scene_color.Load(int3(pixel, 0));
    float straight_depth = scene_depth.Load(int3(pixel, 0));
    float3 straight_point =
        water_opaque_point(pixel, straight_depth, extent, surface, incident);
    float below = max(surface.y - straight_point.y, 0.0);

    // The refracted ray, walked down to the depth the straight one reached. On a
    // flat floor that is where it lands, and the floor reads nearer than it is —
    // the apparent shallowing of a real pool. It is a single step rather than a
    // march, so under a sloping floor the landing is an estimate.
    float3 refracted = water_refract(incident, up, WATER_ETA);
    float3 landing = surface + refracted * (below / max(-refracted.y, 1e-6));
    float4 landing_clip = mul(frame.view_proj, float4(landing, 1.0));

    float4 behind = straight;
    float3 behind_point = straight_point;
    if (landing_clip.w > 0.0)
    {
        float2 landing_ndc = landing_clip.xy / landing_clip.w;
        int2 bent = clamp(int2(float2(landing_ndc.x * 0.5 + 0.5, 0.5 - landing_ndc.y * 0.5)
                               * extent),
                          int2(0, 0), last);
        float bent_depth = scene_depth.Load(int3(bent, 0));
        // **The in-front rejection.** The frame at the landing pixel is only
        // something this surface refracts if it is behind the surface —
        // reversed-Z, so a smaller depth. A post standing out of the water nearer
        // the eye covers the pixels a surface just beyond it bends toward, and
        // reading them would smear the post over the water behind it.
        if (bent_depth < surface_depth)
        {
            behind = scene_color.Load(int3(bent, 0));
            behind_point = water_opaque_point(bent, bent_depth, extent, surface, refracted);
        }
    }

    // --- the medium ---
    float thickness = length(behind_point - surface);
    float3 extinction = medium.absorption.rgb + medium.scattering.rgb;
    float3 transmittance = float3(fog_exp_neg(extinction.r * thickness),
                                  fog_exp_neg(extinction.g * thickness),
                                  fog_exp_neg(extinction.b * thickness));
    // Single scattering of the light arriving at the surface, as a white
    // surface facing up would receive it: the flat ambient, the sky's L1 term
    // and the sun through its cascades. Isotropic — the scattered share of what
    // the ray loses on its way down, `albedo * (1 - T)`, and nothing about which
    // way it leaves.
    float3 albedo = float3(extinction.r > 0.0 ? medium.scattering.r / extinction.r : 0.0,
                           extinction.g > 0.0 ? medium.scattering.g / extinction.g : 0.0,
                           extinction.b > 0.0 ? medium.scattering.b / extinction.b : 0.0);
    float3 to_sun = water.sun_direction.xyz;
    uint cascade;
    float cascade_fade;
    float sun_seen = sun_visibility(surface, to_sun, to_sun.y, up, input.position.xy, cascade,
                                    cascade_fade);
    float3 irradiance = frame.ambient.rgb + sky_irradiance(up)
                      + water.sun_color.rgb * (max(to_sun.y, 0.0) * sun_seen);
    float3 scattered = albedo * irradiance * (1.0 - transmittance);

    // --- the reflection ---
    //
    // The environment `ssr.slang` falls back to for a mirror: the probe grid
    // and the sky, at a roughness of zero and with the whole of a mirror's share
    // on the sky-view LUT.
    float3 mirrored = float3(incident.x, -incident.y, incident.z);
    float3 environment = probe_environment(surface, up, mirrored)
                       + sky_environment(mirrored, 0.0, 1.0);
    float grazing = 1.0 - cos_incident;
    float grazing2 = grazing * grazing;
    float fresnel = WATER_FRESNEL_F0 + (1.0 - WATER_FRESNEL_F0) * (grazing2 * grazing2 * grazing);

    // --- the air in front, which fogs only what this pass adds ---
    //
    // The light transmitted through the water left the opaque frame already
    // fogged, by `mesh.slang` or by `volumetric_composite.slang`; charging it
    // again would fog the floor of a pool twice. The reflected and scattered
    // light start at the surface, so they are fogged here — and the air's own
    // radiance is added in the share of the pixel that light now accounts for,
    // `1 - (1 - F) * T`, since the transmitted term carries the rest of it.
    WaterAir air;
    if (water.froxels.x != 0u)
    {
        air = water_froxel_air(pixel, surface_depth);
    }
    else
    {
        air = water_analytic_air(surface);
    }
    float3 added = environment * fresnel + scattered * (1.0 - fresnel);
    float3 carried = 1.0 - transmittance * (1.0 - fresnel);
    float3 lit = behind.rgb * transmittance * (1.0 - fresnel)
               + added * air.survives + air.inscatter * carried;

    // --- the shoreline ---
    //
    // Two multiplies rather than a `lerp`, on `mesh.slang`'s fog composite's
    // terms: at a fade of one the frame is `lit` bit for bit.
    float fade = saturate(thickness / WATER_SHORE_FADE);
    // Coverage in the alpha: the surface covers its pixel wherever it shows,
    // whatever was behind it. `max` rather than a blend so a pixel over
    // covered ground stays exactly one — the opaque frame's value on every
    // view but a transparent one. See `tonemap.slang`'s `coverage_alpha`.
    return float4(straight.rgb * (1.0 - fade) + lit * fade, max(straight.a, fade));
}
