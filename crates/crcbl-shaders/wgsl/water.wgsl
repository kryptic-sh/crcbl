struct WaterVertex_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) body_0 : vec4<u32>,
};

@binding(4) @group(0) var<storage, read> water_vertices_0 : array<WaterVertex_std430_0>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0
{
    @align(16) data_1 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(2)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E14_0
{
    @align(16) data_2 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(14)>,
};

struct FrameUniforms_std140_0
{
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) camera_position_0 : vec4<f32>,
    @align(16) ambient_0 : vec4<f32>,
    @align(16) shadow_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0,
    @align(16) cascade_far_0 : vec4<f32>,
    @align(16) shadow_params_0 : vec4<f32>,
    @align(16) cluster_grid_0 : vec4<u32>,
    @align(16) light_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E14_0,
    @align(16) probe_counts_0 : vec4<u32>,
    @align(16) probe_levels_0 : vec4<u32>,
    @align(16) probe_level_origin_0 : array<vec4<f32>, i32(4)>,
    @align(16) probe_level_inv_spacing_0 : array<vec4<f32>, i32(4)>,
    @align(16) probe_level_offset_0 : array<vec4<u32>, i32(4)>,
    @align(16) lod_params_0 : vec4<f32>,
    @align(16) fog_params_0 : vec4<f32>,
    @align(16) fog_color_0 : vec4<f32>,
    @align(16) sky_sh_r_0 : vec4<f32>,
    @align(16) sky_sh_g_0 : vec4<f32>,
    @align(16) sky_sh_b_0 : vec4<f32>,
    @align(16) previous_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) vertex_pool_0 : vec4<u32>,
    @align(16) shadow_atlas_rect_0 : array<vec4<f32>, i32(16)>,
    @align(16) shadow_filter_0 : vec4<u32>,
};

@binding(1) @group(0) var<uniform> frame_0 : FrameUniforms_std140_0;
@binding(7) @group(0) var scene_depth_0 : texture_depth_2d;

struct WaterMedium_std430_0
{
    @align(16) absorption_0 : vec4<f32>,
    @align(16) scattering_0 : vec4<f32>,
};

@binding(5) @group(0) var<storage, read> media_0 : array<WaterMedium_std430_0>;

@binding(6) @group(0) var scene_color_0 : texture_2d<f32>;

struct VolumetricParams_std140_0
{
    @align(16) inverse_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) eye_0 : vec4<f32>,
    @align(16) depth_row_0 : vec4<f32>,
    @align(16) fog_params_1 : vec4<f32>,
    @align(16) fog_color_1 : vec4<f32>,
    @align(16) sun_direction_0 : vec4<f32>,
    @align(16) sun_radiance_0 : vec4<f32>,
    @align(16) shadow_view_proj_1 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0,
    @align(16) cascade_far_1 : vec4<f32>,
    @align(16) shadow_params_1 : vec4<f32>,
    @align(16) grid_x_0 : u32,
    @align(4) grid_y_0 : u32,
    @align(8) slices_0 : u32,
    @align(4) tile_pixels_0 : u32,
    @align(16) viewport_x_0 : u32,
    @align(4) viewport_y_0 : u32,
    @align(8) froxel_count_0 : u32,
    @align(4) pad0_0 : u32,
    @align(16) light_view_proj_1 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E14_0,
    @align(16) shadow_atlas_rect_1 : array<vec4<f32>, i32(16)>,
};

@binding(3) @group(0) var<uniform> params_0 : VolumetricParams_std140_0;
struct WaterParams_std140_0
{
    @align(16) sun_direction_1 : vec4<f32>,
    @align(16) sun_color_0 : vec4<f32>,
    @align(16) froxels_0 : vec4<u32>,
};

@binding(0) @group(0) var<uniform> water_0 : WaterParams_std140_0;
@binding(8) @group(0) var shadow_atlas_0 : texture_depth_2d;

@binding(9) @group(0) var shadow_sampler_0 : sampler_comparison;

struct SsrParams_std140_0
{
    @align(16) inv_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) inv_view_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) probe_counts_1 : vec4<u32>,
    @align(16) probe_levels_1 : vec4<u32>,
    @align(16) probe_level_origin_1 : array<vec4<f32>, i32(4)>,
    @align(16) probe_level_inv_spacing_1 : array<vec4<f32>, i32(4)>,
    @align(16) probe_level_offset_1 : array<vec4<u32>, i32(4)>,
    @align(16) hiz_0 : vec4<u32>,
    @align(16) sky_0 : array<vec4<f32>, i32(3)>,
    @align(16) atmosphere_0 : vec4<f32>,
};

@binding(2) @group(0) var<uniform> camera_0 : SsrParams_std140_0;
struct GpuProbe_std430_0
{
    @align(16) sh_r_0 : vec4<f32>,
    @align(16) sh_g_0 : vec4<f32>,
    @align(16) sh_b_0 : vec4<f32>,
};

@binding(10) @group(0) var<storage, read> probes_0 : array<GpuProbe_std430_0>;

@binding(12) @group(0) var probe_visibility_0 : texture_2d_array<f32>;

@binding(11) @group(0) var sky_prefilter_0 : texture_2d<f32>;

@binding(13) @group(0) var<storage, read> sky_view_0 : array<vec4<f32>>;

@binding(14) @group(0) var<storage, read> volumetrics_0 : array<vec4<f32>>;

@binding(15) @group(0) var<storage, read> lighting_0 : array<vec4<f32>>;

var<private> FOG_RATIO_KERNEL_0 : array<f32, i32(5)> = array<f32, i32(5)>( 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f );
var<private> SHADOW_DISC_0 : array<vec2<f32>, i32(32)> = array<vec2<f32>, i32(32)>( vec2<f32>(0.125f, 0.0f), vec2<f32>(-0.15964500606060028f, 0.14624799787998199f), vec2<f32>(0.02443600073456764f, -0.27843800187110901f), vec2<f32>(0.2012220025062561f, 0.26245900988578796f), vec2<f32>(-0.36926800012588501f, -0.06531800329685211f), vec2<f32>(0.34980198740959167f, -0.22251600027084351f), vec2<f32>(-0.11700200289487839f, 0.43524199724197388f), vec2<f32>(-0.22313599288463593f, -0.42963400483131409f), vec2<f32>(0.48411500453948975f, 0.17679800093173981f), vec2<f32>(-0.50364100933074951f, 0.20789599418640137f), vec2<f32>(0.24278800189495087f, -0.51882398128509521f), vec2<f32>(0.17941400408744812f, 0.57200098037719727f), vec2<f32>(-0.54075700044631958f, -0.31338000297546387f), vec2<f32>(0.63437002897262573f, -0.13946400582790375f), vec2<f32>(-0.38714599609375f, 0.55067497491836548f), vec2<f32>(-0.0894400030374527f, -0.69019997119903564f), vec2<f32>(0.5490720272064209f, 0.46275800466537476f), vec2<f32>(-0.73887801170349121f, 0.0305550005286932f), vec2<f32>(0.5389549732208252f, -0.53633201122283936f), vec2<f32>(-0.03605800122022629f, 0.77979201078414917f), vec2<f32>(-0.51281797885894775f, -0.61452698707580566f), vec2<f32>(0.81235998868942261f, 0.10930199921131134f), vec2<f32>(-0.68831098079681396f, 0.47890898585319519f), vec2<f32>(0.18808600306510925f, -0.83606100082397461f), vec2<f32>(0.43503299355506897f, 0.75919097661972046f), vec2<f32>(-0.85044801235198975f, -0.27131599187850952f), vec2<f32>(0.82610201835632324f, -0.38168001174926758f), vec2<f32>(-0.35788801312446594f, 0.85515600442886353f), vec2<f32>(-0.31940698623657227f, -0.88803398609161377f), vec2<f32>(0.84990900754928589f, 0.44668799638748169f), vec2<f32>(-0.94403499364852905f, 0.24884499609470367f), vec2<f32>(0.53659600019454956f, -0.83452999591827393f) );
var<private> SHADOW_PROBE_INDEX_0 : array<u32, i32(5)> = array<u32, i32(5)>( u32(0), u32(23), u32(25), u32(27), u32(29) );
var<private> SHADOW_ROTATIONS_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(1.0f, 0.0f), vec2<f32>(0.92387998104095459f, 0.38268300890922546f), vec2<f32>(0.70710700750350952f, 0.70710700750350952f), vec2<f32>(0.38268300890922546f, 0.92387998104095459f), vec2<f32>(0.0f, 1.0f), vec2<f32>(-0.38268300890922546f, 0.92387998104095459f), vec2<f32>(-0.70710700750350952f, 0.70710700750350952f), vec2<f32>(-0.92387998104095459f, 0.38268300890922546f), vec2<f32>(-1.0f, 0.0f), vec2<f32>(-0.92387998104095459f, -0.38268300890922546f), vec2<f32>(-0.70710700750350952f, -0.70710700750350952f), vec2<f32>(-0.38268300890922546f, -0.92387998104095459f), vec2<f32>(-0.0f, -1.0f), vec2<f32>(0.38268300890922546f, -0.92387998104095459f), vec2<f32>(0.70710700750350952f, -0.70710700750350952f), vec2<f32>(0.92387998104095459f, -0.38268300890922546f) );
var<private> SHADOW_DITHER_0 : array<u32, i32(16)> = array<u32, i32(16)>( u32(0), u32(8), u32(2), u32(10), u32(12), u32(4), u32(14), u32(6), u32(3), u32(11), u32(1), u32(9), u32(15), u32(7), u32(13), u32(5) );
var<private> FOG_KERNEL_0 : array<f32, i32(8)> = array<f32, i32(8)>( 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f );
struct SurfaceOutput_0
{
    @builtin(position) position_1 : vec4<f32>,
    @location(0) world_position_0 : vec3<f32>,
    @interpolate(flat) @location(1) body_1 : u32,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> SurfaceOutput_0
{
    var vertex_0 : WaterVertex_std430_0 = water_vertices_0[index_0];
    var output_0 : SurfaceOutput_0;
    var _S1 : vec3<f32> = vertex_0.position_0.xyz;
    output_0.world_position_0 = _S1;
    output_0.position_1 = (((vec4<f32>(_S1, 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_0[i32(0)][i32(0)], frame_0.view_proj_0.data_0[i32(1)][i32(0)], frame_0.view_proj_0.data_0[i32(2)][i32(0)], frame_0.view_proj_0.data_0[i32(3)][i32(0)], frame_0.view_proj_0.data_0[i32(0)][i32(1)], frame_0.view_proj_0.data_0[i32(1)][i32(1)], frame_0.view_proj_0.data_0[i32(2)][i32(1)], frame_0.view_proj_0.data_0[i32(3)][i32(1)], frame_0.view_proj_0.data_0[i32(0)][i32(2)], frame_0.view_proj_0.data_0[i32(1)][i32(2)], frame_0.view_proj_0.data_0[i32(2)][i32(2)], frame_0.view_proj_0.data_0[i32(3)][i32(2)], frame_0.view_proj_0.data_0[i32(0)][i32(3)], frame_0.view_proj_0.data_0[i32(1)][i32(3)], frame_0.view_proj_0.data_0[i32(2)][i32(3)], frame_0.view_proj_0.data_0[i32(3)][i32(3)]))));
    output_0.body_1 = vertex_0.body_0.x;
    return output_0;
}

fn water_ndc_0( pixel_0 : vec2<i32>,  extent_0 : vec2<f32>) -> vec2<f32>
{
    return vec2<f32>((f32(pixel_0.x) + 0.5f) / extent_0.x * 2.0f - 1.0f, 1.0f - (f32(pixel_0.y) + 0.5f) / extent_0.y * 2.0f);
}

fn volumetric_unproject_0( ndc_0 : vec2<f32>,  depth_0 : f32) -> vec3<f32>
{
    var world_0 : vec4<f32> = (((vec4<f32>(ndc_0, depth_0, 1.0f)) * (mat4x4<f32>(params_0.inverse_view_proj_0.data_0[i32(0)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(3)]))));
    return world_0.xyz / vec3<f32>(world_0.w);
}

fn water_opaque_point_0( pixel_1 : vec2<i32>,  depth_1 : f32,  extent_1 : vec2<f32>,  surface_0 : vec3<f32>,  direction_0 : vec3<f32>) -> vec3<f32>
{
    if(depth_1 <= 0.0f)
    {
        return surface_0 + direction_0 * vec3<f32>(1000.0f);
    }
    return volumetric_unproject_0(water_ndc_0(pixel_1, extent_1), depth_1);
}

fn water_refract_0( incident_0 : vec3<f32>,  normal_0 : vec3<f32>,  eta_0 : f32) -> vec3<f32>
{
    var cos_incident_0 : f32 = - dot(incident_0, normal_0);
    return incident_0 * vec3<f32>(eta_0) + normal_0 * vec3<f32>((eta_0 * cos_incident_0 - sqrt(max(1.0f - eta_0 * eta_0 * (1.0f - cos_incident_0 * cos_incident_0), 0.0f))));
}

fn fog_exp_neg_0( x_0 : f32) -> f32
{
    var clamped_0 : f32 = clamp(x_0, -87.0f, 87.0f);
    var n_0 : f32 = floor(clamped_0 * 1.4426950216293335f + 0.5f);
    var _S2 : f32 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);
    var kernel_0 : f32 = 0.0001984127011383f;
    var term_0 : i32 = i32(6);
    for(;;)
    {
        if(term_0 >= i32(0))
        {
        }
        else
        {
            break;
        }
        var _S3 : f32 = kernel_0 * _S2 + FOG_KERNEL_0[term_0];
        var term_1 : i32 = term_0 - i32(1);
        kernel_0 = _S3;
        term_0 = term_1;
    }
    return kernel_0 * (bitcast<f32>(((u32(i32(127) - i32(n_0)) << (u32(23))))));
}

fn atlas_rect_0( tile_0 : u32) -> vec4<f32>
{
    return frame_0.shadow_atlas_rect_0[tile_0];
}

fn atlas_rect_is_empty_0( rect_0 : vec4<f32>) -> bool
{
    return !((rect_0.x) > 0.0f);
}

fn tile_texels_0( rect_1 : vec4<f32>) -> f32
{
    return rect_1.x / frame_0.shadow_params_0.x;
}

fn shadow_normal_offset_0( geometric_normal_0 : vec3<f32>,  to_light_0 : vec3<f32>) -> f32
{
    var cosine_0 : f32 = saturate(dot(geometric_normal_0, to_light_0));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}

fn shadow_filter_mode_0( pixel_2 : vec2<f32>) -> u32
{
    var _S4 : u32;
    if(u32(pixel_2.x) < (frame_0.shadow_filter_0.z))
    {
        _S4 = frame_0.shadow_filter_0.x;
    }
    else
    {
        _S4 = frame_0.shadow_filter_0.y;
    }
    return _S4;
}

fn atlas_step_0( rect_2 : vec4<f32>) -> vec2<f32>
{
    return frame_0.shadow_params_0.xy / rect_2.xy;
}

fn atlas_uv_0( rect_3 : vec4<f32>,  tile_uv_0 : vec2<f32>) -> vec2<f32>
{
    return rect_3.zw + tile_uv_0 * rect_3.xy;
}

fn tile_tap_0( rect_4 : vec4<f32>,  texel_step_0 : vec2<f32>,  tile_uv_1 : vec2<f32>,  spoke_0 : vec2<f32>,  rotation_0 : vec2<f32>,  reference_0 : f32) -> f32
{
    var tile_min_0 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_step_0;
    var _S5 : f32 = spoke_0.x;
    var _S6 : f32 = rotation_0.x;
    var _S7 : f32 = spoke_0.y;
    var _S8 : f32 = rotation_0.y;
    return (textureSampleCompareLevel((shadow_atlas_0), (shadow_sampler_0), (atlas_uv_0(rect_4, clamp(tile_uv_1 + vec2<f32>(_S5 * _S6 - _S7 * _S8, _S5 * _S8 + _S7 * _S6) * texel_step_0, tile_min_0, vec2<f32>(1.0f) - tile_min_0))), (reference_0)));
}

fn tile_box_pcf_0( tile_1 : u32,  tile_uv_2 : vec2<f32>,  reference_1 : f32) -> f32
{
    var rect_5 : vec4<f32> = atlas_rect_0(tile_1);
    if(atlas_rect_is_empty_0(rect_5))
    {
        return 1.0f;
    }
    var _S9 : vec2<f32> = atlas_step_0(rect_5);
    var y_0 : i32 = i32(-1);
    var visibility_0 : f32 = 0.0f;
    for(;;)
    {
        if(y_0 <= i32(1))
        {
        }
        else
        {
            break;
        }
        var x_1 : i32 = i32(-1);
        for(;;)
        {
            if(x_1 <= i32(1))
            {
            }
            else
            {
                break;
            }
            var visibility_1 : f32 = visibility_0 + tile_tap_0(rect_5, _S9, tile_uv_2, vec2<f32>(f32(x_1), f32(y_0)), vec2<f32>(1.0f, 0.0f), reference_1);
            x_1 = x_1 + i32(1);
            visibility_0 = visibility_1;
        }
        y_0 = y_0 + i32(1);
    }
    return visibility_0 / 9.0f;
}

fn shadow_rotation_0( pixel_3 : vec2<f32>) -> vec2<f32>
{
    var cell_0 : vec2<u32> = (vec2<u32>(pixel_3) & (vec2<u32>(u32(3))));
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * u32(4) + cell_0.x]];
}

fn tile_pcf_0( tile_2 : u32,  tile_uv_3 : vec2<f32>,  reference_2 : f32,  pixel_4 : vec2<f32>,  radius_0 : f32) -> f32
{
    var rotation_1 : vec2<f32> = shadow_rotation_0(pixel_4);
    var rect_6 : vec4<f32> = atlas_rect_0(tile_2);
    if(atlas_rect_is_empty_0(rect_6))
    {
        return 1.0f;
    }
    var texel_step_1 : vec2<f32> = atlas_step_0(rect_6);
    var probe_taps_0 : array<f32, i32(5)>;
    var spot_0 : u32 = u32(0);
    var probe_0 : f32 = 0.0f;
    for(;;)
    {
        if(spot_0 < u32(5))
        {
        }
        else
        {
            break;
        }
        var _S10 : f32 = tile_tap_0(rect_6, texel_step_1, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * vec2<f32>(radius_0), rotation_1, reference_2);
        probe_taps_0[spot_0] = _S10;
        var probe_1 : f32 = probe_0 + _S10;
        spot_0 = spot_0 + u32(1);
        probe_0 = probe_1;
    }
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }
    var index_1 : u32 = u32(1);
    var visibility_2 : f32 = probe_taps_0[i32(0)];
    for(;;)
    {
        if(index_1 < u32(23))
        {
        }
        else
        {
            break;
        }
        var visibility_3 : f32 = visibility_2 + tile_tap_0(rect_6, texel_step_1, tile_uv_3, SHADOW_DISC_0[index_1] * vec2<f32>(radius_0), rotation_1, reference_2);
        index_1 = index_1 + u32(1);
        visibility_2 = visibility_3;
    }
    var _S11 : vec2<f32> = vec2<f32>(radius_0);
    var visibility_4 : f32 = visibility_2 + probe_taps_0[i32(1)] + tile_tap_0(rect_6, texel_step_1, tile_uv_3, vec2<f32>(0.43503299355506897f, 0.75919097661972046f) * _S11, rotation_1, reference_2) + probe_taps_0[i32(2)] + tile_tap_0(rect_6, texel_step_1, tile_uv_3, vec2<f32>(0.82610201835632324f, -0.38168001174926758f) * _S11, rotation_1, reference_2) + probe_taps_0[i32(3)] + tile_tap_0(rect_6, texel_step_1, tile_uv_3, vec2<f32>(-0.31940698623657227f, -0.88803398609161377f) * _S11, rotation_1, reference_2) + probe_taps_0[i32(4)];
    index_1 = u32(30);
    visibility_2 = visibility_4;
    for(;;)
    {
        if(index_1 < u32(32))
        {
        }
        else
        {
            break;
        }
        var visibility_5 : f32 = visibility_2 + tile_tap_0(rect_6, texel_step_1, tile_uv_3, SHADOW_DISC_0[index_1] * _S11, rotation_1, reference_2);
        index_1 = index_1 + u32(1);
        visibility_2 = visibility_5;
    }
    return visibility_2 / 32.0f;
}

fn sun_penumbra_texels_0( cascade_0 : u32,  tile_uv_4 : vec2<f32>,  reference_3 : f32,  rotation_2 : vec2<f32>) -> f32
{
    var rect_7 : vec4<f32> = atlas_rect_0(cascade_0);
    var texel_step_2 : vec2<f32> = atlas_step_0(rect_7);
    var _S12 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_step_2;
    const _S13 : vec2<f32> = vec2<f32>(1.0f, 1.0f);
    var _S14 : vec2<f32> = _S13 / frame_0.shadow_params_0.xy;
    var _S15 : f32 = rotation_2.x;
    var _S16 : f32 = rotation_2.y;
    var _S17 : vec2<f32> = vec2<f32>(1.0f) - _S12;
    var _S18 : vec2<f32> = _S14 - _S13;
    var _S19 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(1.41421604156494141f * _S15 - 0.0f * _S16, 1.41421604156494141f * _S16 + 0.0f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_2 : f32 = (textureLoad((shadow_atlas_0), ((_S19)).xy, ((_S19)).z));
    var sum_0 : f32;
    var found_0 : f32;
    if(depth_2 > reference_3)
    {
        sum_0 = depth_2;
        found_0 = 1.0f;
    }
    else
    {
        sum_0 = 0.0f;
        found_0 = 0.0f;
    }
    var _S20 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(-1.80617594718933105f * _S15 - 1.65460801124572754f * _S16, -1.80617594718933105f * _S16 + 1.65460801124572754f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_3 : f32 = (textureLoad((shadow_atlas_0), ((_S20)).xy, ((_S20)).z));
    if(depth_3 > reference_3)
    {
        var found_1 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_3;
        found_0 = found_1;
    }
    var _S21 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(0.27646398544311523f * _S15 - -3.15016794204711914f * _S16, 0.27646398544311523f * _S16 + -3.15016794204711914f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_4 : f32 = (textureLoad((shadow_atlas_0), ((_S21)).xy, ((_S21)).z));
    if(depth_4 > reference_3)
    {
        var found_2 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_4;
        found_0 = found_2;
    }
    var _S22 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(2.27656793594360352f * _S15 - 2.96938395500183105f * _S16, 2.27656793594360352f * _S16 + 2.96938395500183105f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_5 : f32 = (textureLoad((shadow_atlas_0), ((_S22)).xy, ((_S22)).z));
    if(depth_5 > reference_3)
    {
        var found_3 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_5;
        found_0 = found_3;
    }
    var _S23 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(-4.17778396606445312f * _S15 - -0.73899197578430176f * _S16, -4.17778396606445312f * _S16 + -0.73899197578430176f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_6 : f32 = (textureLoad((shadow_atlas_0), ((_S23)).xy, ((_S23)).z));
    if(depth_6 > reference_3)
    {
        var found_4 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_6;
        found_0 = found_4;
    }
    var _S24 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(3.95756006240844727f * _S15 - -2.51747989654541016f * _S16, 3.95756006240844727f * _S16 + -2.51747989654541016f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_7 : f32 = (textureLoad((shadow_atlas_0), ((_S24)).xy, ((_S24)).z));
    if(depth_7 > reference_3)
    {
        var found_5 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_7;
        found_0 = found_5;
    }
    var _S25 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(-1.32372796535491943f * _S15 - 4.92420005798339844f * _S16, -1.32372796535491943f * _S16 + 4.92420005798339844f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_8 : f32 = (textureLoad((shadow_atlas_0), ((_S25)).xy, ((_S25)).z));
    if(depth_8 > reference_3)
    {
        var found_6 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_8;
        found_0 = found_6;
    }
    var _S26 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(-2.52448797225952148f * _S15 - -4.86075210571289062f * _S16, -2.52448797225952148f * _S16 + -4.86075210571289062f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_9 : f32 = (textureLoad((shadow_atlas_0), ((_S26)).xy, ((_S26)).z));
    if(depth_9 > reference_3)
    {
        var found_7 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_9;
        found_0 = found_7;
    }
    var _S27 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(5.47713613510131836f * _S15 - 2.00024008750915527f * _S16, 5.47713613510131836f * _S16 + 2.00024008750915527f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_10 : f32 = (textureLoad((shadow_atlas_0), ((_S27)).xy, ((_S27)).z));
    if(depth_10 > reference_3)
    {
        var found_8 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_10;
        found_0 = found_8;
    }
    var _S28 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(-5.69804811477661133f * _S15 - 2.35207200050354004f * _S16, -5.69804811477661133f * _S16 + 2.35207200050354004f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_11 : f32 = (textureLoad((shadow_atlas_0), ((_S28)).xy, ((_S28)).z));
    if(depth_11 > reference_3)
    {
        var found_9 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_11;
        found_0 = found_9;
    }
    var _S29 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(2.74683189392089844f * _S15 - -5.86983203887939453f * _S16, 2.74683189392089844f * _S16 + -5.86983203887939453f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_12 : f32 = (textureLoad((shadow_atlas_0), ((_S29)).xy, ((_S29)).z));
    if(depth_12 > reference_3)
    {
        var found_10 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_12;
        found_0 = found_10;
    }
    var _S30 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(2.02983999252319336f * _S15 - 6.47145605087280273f * _S16, 2.02983999252319336f * _S16 + 6.47145605087280273f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_13 : f32 = (textureLoad((shadow_atlas_0), ((_S30)).xy, ((_S30)).z));
    if(depth_13 > reference_3)
    {
        var found_11 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_13;
        found_0 = found_11;
    }
    var _S31 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(-6.11796808242797852f * _S15 - -3.54548811912536621f * _S16, -6.11796808242797852f * _S16 + -3.54548811912536621f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_14 : f32 = (textureLoad((shadow_atlas_0), ((_S31)).xy, ((_S31)).z));
    if(depth_14 > reference_3)
    {
        var found_12 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_14;
        found_0 = found_12;
    }
    var _S32 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(7.17707204818725586f * _S15 - -1.57785594463348389f * _S16, 7.17707204818725586f * _S16 + -1.57785594463348389f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_15 : f32 = (textureLoad((shadow_atlas_0), ((_S32)).xy, ((_S32)).z));
    if(depth_15 > reference_3)
    {
        var found_13 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_15;
        found_0 = found_13;
    }
    var _S33 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(-4.38005590438842773f * _S15 - 6.23017597198486328f * _S16, -4.38005590438842773f * _S16 + 6.23017597198486328f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_16 : f32 = (textureLoad((shadow_atlas_0), ((_S33)).xy, ((_S33)).z));
    if(depth_16 > reference_3)
    {
        var found_14 : f32 = found_0 + 1.0f;
        sum_0 = sum_0 + depth_16;
        found_0 = found_14;
    }
    var _S34 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(-1.01189601421356201f * _S15 - -7.80872011184692383f * _S16, -1.01189601421356201f * _S16 + -7.80872011184692383f * _S15) * texel_step_2, _S12, _S17)) * _S14, _S18)), i32(0));
    var depth_17 : f32 = (textureLoad((shadow_atlas_0), ((_S34)).xy, ((_S34)).z));
    if(depth_17 > reference_3)
    {
        var sum_1 : f32 = sum_0 + depth_17;
        found_0 = found_0 + 1.0f;
        sum_0 = sum_1;
    }
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }
    var _S35 : f32 = 2.0f * frame_0.cascade_far_0[cascade_0];
    return clamp((sum_0 / found_0 - reference_3) * (_S35 + 40.0f) * 0.01999999955296516f / (_S35 / tile_texels_0(rect_7)), 2.0f, 8.0f);
}

fn cascade_visibility_0( cascade_1 : u32,  world_position_1 : vec3<f32>,  to_light_1 : vec3<f32>,  geometric_normal_1 : vec3<f32>,  pixel_5 : vec2<f32>) -> f32
{
    var rect_8 : vec4<f32> = atlas_rect_0(cascade_1);
    if(atlas_rect_is_empty_0(rect_8))
    {
        return 1.0f;
    }
    var texel_world_0 : f32 = 2.0f * frame_0.cascade_far_0[cascade_1] / tile_texels_0(rect_8);
    var clip_0 : vec4<f32> = (((vec4<f32>(world_position_1 + geometric_normal_1 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_1))) + to_light_1 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.z)), 1.0f)) * (mat4x4<f32>(frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(3)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(3)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(3)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(3)]))));
    var ndc_1 : vec3<f32> = clip_0.xyz / vec3<f32>(clip_0.w);
    var _S36 : bool;
    if((any(((abs(ndc_1.xy)) > vec2<f32>(1.0f)))))
    {
        _S36 = true;
    }
    else
    {
        _S36 = (ndc_1.z) <= 0.0f;
    }
    if(_S36)
    {
        return 1.0f;
    }
    var tile_uv_5 : vec2<f32> = vec2<f32>(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);
    var mode_0 : u32 = shadow_filter_mode_0(pixel_5);
    if(mode_0 == u32(2))
    {
        return tile_box_pcf_0(cascade_1, tile_uv_5, ndc_1.z);
    }
    if(mode_0 == u32(1))
    {
        return tile_pcf_0(cascade_1, tile_uv_5, ndc_1.z, pixel_5, 2.0f);
    }
    var _S37 : f32 = ndc_1.z;
    return tile_pcf_0(cascade_1, tile_uv_5, _S37, pixel_5, sun_penumbra_texels_0(cascade_1, tile_uv_5, _S37, shadow_rotation_0(pixel_5)));
}

fn sun_visibility_0( world_position_2 : vec3<f32>,  to_light_2 : vec3<f32>,  n_dot_l_0 : f32,  geometric_normal_2 : vec3<f32>,  pixel_6 : vec2<f32>,  selected_0 : ptr<function, u32>,  fade_0 : ptr<function, f32>) -> f32
{
    var cascade_2 : u32;
    var covered_0 : bool;
    (*selected_0) = u32(2);
    (*fade_0) = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }
    var eye_distance_0 : f32 = length(world_position_2 - frame_0.camera_position_0.xyz);
    var index_2 : u32 = u32(0);
    for(;;)
    {
        if(index_2 < u32(2))
        {
        }
        else
        {
            covered_0 = false;
            cascade_2 = u32(1);
            break;
        }
        if(eye_distance_0 < (frame_0.cascade_far_0[index_2]))
        {
            covered_0 = true;
            cascade_2 = index_2;
            break;
        }
        index_2 = index_2 + u32(1);
    }
    if(covered_0)
    {
        (*selected_0) = cascade_2;
    }
    var visibility_6 : f32 = cascade_visibility_0(cascade_2, world_position_2, to_light_2, geometric_normal_2, pixel_6);
    var _S38 : u32 = cascade_2 + u32(1);
    if(_S38 >= u32(2))
    {
        return visibility_6;
    }
    var band_0 : f32 = frame_0.cascade_far_0[cascade_2] * 0.10000000149011612f;
    var blend_0 : f32 = saturate((eye_distance_0 - (frame_0.cascade_far_0[cascade_2] - band_0)) / band_0);
    (*fade_0) = blend_0;
    if(blend_0 <= 0.0f)
    {
        return visibility_6;
    }
    return mix(visibility_6, cascade_visibility_0(_S38, world_position_2, to_light_2, geometric_normal_2, pixel_6), blend_0);
}

fn sky_irradiance_0( normal_1 : vec3<f32>) -> vec3<f32>
{
    var basis_0 : vec4<f32> = vec4<f32>(normal_1, 1.0f);
    return max(vec3<f32>(dot(frame_0.sky_sh_r_0, basis_0), dot(frame_0.sky_sh_g_0, basis_0), dot(frame_0.sky_sh_b_0, basis_0)), vec3<f32>(0.0f, 0.0f, 0.0f));
}

fn probe_level_reach_0( world_position_3 : vec3<f32>,  origin_0 : vec3<f32>,  inv_spacing_0 : vec3<f32>,  last_0 : vec3<f32>) -> f32
{
    var reach_0 : f32 = 0.0f;
    var axis_0 : u32 = u32(0);
    for(;;)
    {
        if(axis_0 < u32(3))
        {
        }
        else
        {
            break;
        }
        var _S39 : u32 = axis_0;
        var _S40 : bool;
        if((last_0[axis_0]) == 0.0f)
        {
            _S40 = true;
        }
        else
        {
            _S40 = (inv_spacing_0[axis_0]) == 0.0f;
        }
        if(_S40)
        {
            axis_0 = axis_0 + u32(1);
            continue;
        }
        reach_0 = max(reach_0, abs(2.0f * ((world_position_3[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S39] - 1.0f));
        axis_0 = axis_0 + u32(1);
    }
    return reach_0;
}

fn probe_level_of_0( reach_1 : f32,  levels_0 : u32) -> vec2<f32>
{
    var level_0 : u32 = u32(0);
    for(;;)
    {
        var _S41 : u32 = level_0 + u32(1);
        if(_S41 < levels_0)
        {
        }
        else
        {
            break;
        }
        var _S42 : f32 = f32(level_0);
        var at_0 : f32 = reach_1 * exp2(- _S42);
        if(at_0 < 1.0f)
        {
            return vec2<f32>(_S42, saturate((1.0f - at_0) / 0.25f));
        }
        level_0 = _S41;
    }
    return vec2<f32>(f32(levels_0 - u32(1)), 1.0f);
}

fn probe_wrap_0( cell_1 : u32,  offset_0 : u32,  count_0 : u32) -> u32
{
    var at_1 : u32 = cell_1 + offset_0;
    var _S43 : u32;
    if(at_1 >= count_0)
    {
        _S43 = at_1 - count_0;
    }
    else
    {
        _S43 = at_1;
    }
    return _S43;
}

fn probe_row_0( level_1 : u32,  cell_2 : vec3<u32>) -> u32
{
    var counts_0 : vec3<u32> = camera_0.probe_counts_1.xyz;
    var offset_1 : vec3<u32> = camera_0.probe_level_offset_1[level_1].xyz;
    var _S44 : u32 = counts_0.x;
    var _S45 : u32 = counts_0.y;
    return min(camera_0.probe_levels_1.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S45 + probe_wrap_0(cell_2.y, offset_1.y, _S45)) * _S44 + probe_wrap_0(cell_2.x, offset_1.x, _S44), max(camera_0.probe_counts_1.w, u32(1)) - u32(1));
}

fn sign_not_zero_0( value_0 : f32) -> f32
{
    var _S46 : f32;
    if(value_0 >= 0.0f)
    {
        _S46 = 1.0f;
    }
    else
    {
        _S46 = -1.0f;
    }
    return _S46;
}

fn oct_encode_0( direction_1 : vec3<f32>) -> vec2<f32>
{
    var _S47 : f32 = direction_1.y;
    var p_0 : vec2<f32> = direction_1.xz / vec2<f32>(max(abs(direction_1.x) + abs(_S47) + abs(direction_1.z), 9.99999968265522539e-21f));
    var p_1 : vec2<f32>;
    if(_S47 < 0.0f)
    {
        var _S48 : f32 = p_0.y;
        var _S49 : f32 = p_0.x;
        p_1 = vec2<f32>((1.0f - abs(_S48)) * sign_not_zero_0(_S49), (1.0f - abs(_S49)) * sign_not_zero_0(_S48));
    }
    else
    {
        p_1 = p_0;
    }
    return p_1;
}

fn probe_moments_0( index_3 : u32,  direction_2 : vec3<f32>) -> vec2<f32>
{
    var width_0 : u32;
    var height_0 : u32;
    var layers_0 : u32;
    {var dim = textureDimensions((probe_visibility_0));((width_0)) = dim.x;((height_0)) = dim.y;((layers_0)) = textureNumLayers((probe_visibility_0));};
    var _S50 : vec2<f32> = vec2<f32>(0.5f);
    var _S51 : vec2<f32> = vec2<f32>(1.0f);
    var scaled_0 : vec2<f32> = (oct_encode_0(direction_2) * _S50 + _S50) * vec2<f32>(16.0f) + _S51 - _S50;
    var _S52 : vec2<f32> = vec2<f32>(f32(width_0), f32(height_0)) - _S51;
    var low_0 : vec2<f32> = clamp(floor(scaled_0), vec2<f32>(0.0f, 0.0f), _S52);
    var high_0 : vec2<f32> = min(low_0 + _S51, _S52);
    var weight_0 : vec2<f32> = clamp(scaled_0 - low_0, vec2<f32>(0.0f), vec2<f32>(1.0f));
    var layer_0 : i32 = i32(min(index_3, max(layers_0, u32(1)) - u32(1)));
    var _S53 : i32 = i32(low_0.x);
    var _S54 : i32 = i32(low_0.y);
    var _S55 : vec4<i32> = vec4<i32>(_S53, _S54, layer_0, i32(0));
    var _S56 : i32 = i32(high_0.x);
    var _S57 : vec4<i32> = vec4<i32>(_S56, _S54, layer_0, i32(0));
    var _S58 : i32 = i32(high_0.y);
    var _S59 : vec4<i32> = vec4<i32>(_S53, _S58, layer_0, i32(0));
    var _S60 : vec4<i32> = vec4<i32>(_S56, _S58, layer_0, i32(0));
    var _S61 : vec2<f32> = vec2<f32>(weight_0.x);
    return mix(mix((textureLoad((probe_visibility_0), ((_S55)).xy, i32(((_S55)).z), ((_S55)).w)).xy, (textureLoad((probe_visibility_0), ((_S57)).xy, i32(((_S57)).z), ((_S57)).w)).xy, _S61), mix((textureLoad((probe_visibility_0), ((_S59)).xy, i32(((_S59)).z), ((_S59)).w)).xy, (textureLoad((probe_visibility_0), ((_S60)).xy, i32(((_S60)).z), ((_S60)).w)).xy, _S61), vec2<f32>(weight_0.y));
}

fn probe_chebyshev_0( index_4 : u32,  probe_position_0 : vec3<f32>,  world_position_4 : vec3<f32>,  normal_2 : vec3<f32>) -> f32
{
    var to_probe_0 : vec3<f32> = probe_position_0 - (world_position_4 + normal_2 * vec3<f32>(0.05000000074505806f));
    var to_surface_0 : f32 = length(to_probe_0);
    var moments_0 : vec2<f32> = probe_moments_0(index_4, (vec3<f32>(0) - to_probe_0));
    var _S62 : f32 = moments_0.x;
    var _S63 : f32 = max(moments_0.y - _S62 * _S62, 0.0f);
    var behind_0 : f32 = to_surface_0 - _S62;
    var bound_0 : f32 = _S63 / (_S63 + behind_0 * behind_0);
    var _S64 : f32;
    if(to_surface_0 <= _S62)
    {
        _S64 = 1.0f;
    }
    else
    {
        _S64 = bound_0 * bound_0 * bound_0;
    }
    return _S64;
}

fn probe_weight_0( index_5 : u32,  probe_position_1 : vec3<f32>,  world_position_5 : vec3<f32>,  normal_3 : vec3<f32>) -> f32
{
    return max(probe_chebyshev_0(index_5, probe_position_1, world_position_5, normal_3), 0.00009999999747379f);
}

struct GpuProbe_0
{
     sh_r_0 : vec4<f32>,
     sh_g_0 : vec4<f32>,
     sh_b_0 : vec4<f32>,
};

struct WeightedProbe_0
{
     sh_0 : GpuProbe_0,
     weight_1 : f32,
};

fn probe_corner_0( level_2 : u32,  cell_3 : vec3<u32>,  origin_1 : vec3<f32>,  spacing_0 : vec3<f32>,  world_position_6 : vec3<f32>,  normal_4 : vec3<f32>) -> WeightedProbe_0
{
    var row_0 : u32 = probe_row_0(level_2, cell_3);
    var stored_0 : GpuProbe_std430_0 = probes_0[row_0];
    var weight_2 : f32 = probe_weight_0(row_0, origin_1 + vec3<f32>(cell_3) * spacing_0, world_position_6, normal_4);
    var corner_0 : WeightedProbe_0;
    var _S65 : vec4<f32> = vec4<f32>(weight_2);
    corner_0.sh_0.sh_r_0 = stored_0.sh_r_0 * _S65;
    corner_0.sh_0.sh_g_0 = stored_0.sh_g_0 * _S65;
    corner_0.sh_0.sh_b_0 = stored_0.sh_b_0 * _S65;
    corner_0.weight_1 = weight_2;
    return corner_0;
}

fn lerp_probe_0( a_0 : WeightedProbe_0,  b_0 : WeightedProbe_0,  t_0 : f32) -> WeightedProbe_0
{
    var blended_0 : WeightedProbe_0;
    var _S66 : vec4<f32> = vec4<f32>(t_0);
    blended_0.sh_0.sh_r_0 = mix(a_0.sh_0.sh_r_0, b_0.sh_0.sh_r_0, _S66);
    blended_0.sh_0.sh_g_0 = mix(a_0.sh_0.sh_g_0, b_0.sh_0.sh_g_0, _S66);
    blended_0.sh_0.sh_b_0 = mix(a_0.sh_0.sh_b_0, b_0.sh_0.sh_b_0, _S66);
    blended_0.weight_1 = mix(a_0.weight_1, b_0.weight_1, t_0);
    return blended_0;
}

fn probe_level_environment_0( level_3 : u32,  world_position_7 : vec3<f32>,  normal_5 : vec3<f32>,  direction_3 : vec3<f32>) -> vec3<f32>
{
    var _S67 : vec3<f32> = vec3<f32>(1.0f);
    const _S68 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var last_1 : vec3<f32> = max(vec3<f32>(camera_0.probe_counts_1.xyz) - _S67, _S68);
    var origin_2 : vec3<f32> = camera_0.probe_level_origin_1[level_3].xyz;
    var inv_0 : vec3<f32> = camera_0.probe_level_inv_spacing_1[level_3].xyz;
    var grid_0 : vec3<f32> = clamp((world_position_7 - origin_2) * inv_0, _S68, last_1);
    var base_0 : vec3<f32> = floor(grid_0);
    var f_0 : vec3<f32> = grid_0 - base_0;
    var _S69 : vec3<u32> = vec3<u32>(base_0);
    var _S70 : vec3<u32> = vec3<u32>(min(base_0 + _S67, last_1));
    var _S71 : f32 = inv_0.x;
    var _S72 : f32;
    if(_S71 != 0.0f)
    {
        _S72 = 1.0f / _S71;
    }
    else
    {
        _S72 = 0.0f;
    }
    var _S73 : f32 = inv_0.y;
    var _S74 : f32;
    if(_S73 != 0.0f)
    {
        _S74 = 1.0f / _S73;
    }
    else
    {
        _S74 = 0.0f;
    }
    var _S75 : f32 = inv_0.z;
    var _S76 : f32;
    if(_S75 != 0.0f)
    {
        _S76 = 1.0f / _S75;
    }
    else
    {
        _S76 = 0.0f;
    }
    var spacing_1 : vec3<f32> = vec3<f32>(_S72, _S74, _S76);
    var _S77 : u32 = _S69.x;
    var _S78 : u32 = _S69.y;
    var _S79 : u32 = _S69.z;
    var _S80 : u32 = _S70.x;
    var _S81 : f32 = f_0.x;
    var _S82 : u32 = _S70.y;
    var _S83 : u32 = _S70.z;
    var _S84 : f32 = f_0.y;
    var cell_4 : WeightedProbe_0 = lerp_probe_0(lerp_probe_0(lerp_probe_0(probe_corner_0(level_3, vec3<u32>(_S77, _S78, _S79), origin_2, spacing_1, world_position_7, normal_5), probe_corner_0(level_3, vec3<u32>(_S80, _S78, _S79), origin_2, spacing_1, world_position_7, normal_5), _S81), lerp_probe_0(probe_corner_0(level_3, vec3<u32>(_S77, _S82, _S79), origin_2, spacing_1, world_position_7, normal_5), probe_corner_0(level_3, vec3<u32>(_S80, _S82, _S79), origin_2, spacing_1, world_position_7, normal_5), _S81), _S84), lerp_probe_0(lerp_probe_0(probe_corner_0(level_3, vec3<u32>(_S77, _S78, _S83), origin_2, spacing_1, world_position_7, normal_5), probe_corner_0(level_3, vec3<u32>(_S80, _S78, _S83), origin_2, spacing_1, world_position_7, normal_5), _S81), lerp_probe_0(probe_corner_0(level_3, vec3<u32>(_S77, _S82, _S83), origin_2, spacing_1, world_position_7, normal_5), probe_corner_0(level_3, vec3<u32>(_S80, _S82, _S83), origin_2, spacing_1, world_position_7, normal_5), _S81), _S84), f_0.z);
    var _S85 : vec3<f32> = vec3<f32>(2.09439516067504883f);
    return max(vec3<f32>(dot(cell_4.sh_0.sh_r_0.xyz / _S85, direction_3) + cell_4.sh_0.sh_r_0.w / 3.14159274101257324f, dot(cell_4.sh_0.sh_g_0.xyz / _S85, direction_3) + cell_4.sh_0.sh_g_0.w / 3.14159274101257324f, dot(cell_4.sh_0.sh_b_0.xyz / _S85, direction_3) + cell_4.sh_0.sh_b_0.w / 3.14159274101257324f) / vec3<f32>(cell_4.weight_1), _S68);
}

fn probe_environment_0( world_position_8 : vec3<f32>,  normal_6 : vec3<f32>,  direction_4 : vec3<f32>) -> vec3<f32>
{
    if((camera_0.probe_counts_1.w) == u32(0))
    {
        return vec3<f32>(0.0f, 0.0f, 0.0f);
    }
    var pick_0 : vec2<f32> = probe_level_of_0(probe_level_reach_0(world_position_8, camera_0.probe_level_origin_1[i32(0)].xyz, camera_0.probe_level_inv_spacing_1[i32(0)].xyz, max(vec3<f32>(camera_0.probe_counts_1.xyz) - vec3<f32>(1.0f), vec3<f32>(0.0f, 0.0f, 0.0f))), clamp(camera_0.probe_levels_1.x, u32(1), u32(4)));
    var level_4 : u32 = u32(pick_0.x);
    var share_0 : f32 = pick_0.y;
    var fine_0 : vec3<f32> = probe_level_environment_0(level_4, world_position_8, normal_6, direction_4);
    if(share_0 >= 1.0f)
    {
        return fine_0;
    }
    return probe_level_environment_0(level_4 + u32(1), world_position_8, normal_6, direction_4) * vec3<f32>((1.0f - share_0)) + fine_0 * vec3<f32>(share_0);
}

fn decode_fixed_pair_0( texel_0 : vec4<f32>) -> vec2<f32>
{
    return vec2<f32>(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / vec2<f32>(65535.0f);
}

fn fixed_pair_at_0( table_0 : texture_2d<f32>,  at_2 : vec2<f32>) -> vec2<f32>
{
    var width_1 : u32;
    var height_1 : u32;
    {var dim = textureDimensions((table_0));((width_1)) = dim.x;((height_1)) = dim.y;};
    var extent_2 : vec2<f32> = vec2<f32>(f32(width_1), f32(height_1));
    var scaled_1 : vec2<f32> = saturate(at_2) * extent_2 - vec2<f32>(0.5f);
    var _S86 : vec2<f32> = vec2<f32>(1.0f);
    var _S87 : vec2<f32> = extent_2 - _S86;
    var low_1 : vec2<f32> = clamp(floor(scaled_1), vec2<f32>(0.0f, 0.0f), _S87);
    var weight_3 : vec2<f32> = clamp(scaled_1 - low_1, vec2<f32>(0.0f), vec2<f32>(1.0f));
    var _S88 : vec2<i32> = vec2<i32>(low_1);
    var _S89 : vec2<i32> = vec2<i32>(min(low_1 + _S86, _S87));
    var _S90 : i32 = _S88.x;
    var _S91 : i32 = _S88.y;
    var _S92 : vec3<i32> = vec3<i32>(_S90, _S91, i32(0));
    var _S93 : i32 = _S89.x;
    var _S94 : vec3<i32> = vec3<i32>(_S93, _S91, i32(0));
    var _S95 : vec2<f32> = vec2<f32>(weight_3.x);
    var _S96 : i32 = _S89.y;
    var _S97 : vec3<i32> = vec3<i32>(_S90, _S96, i32(0));
    var _S98 : vec3<i32> = vec3<i32>(_S93, _S96, i32(0));
    return mix(mix(decode_fixed_pair_0((textureLoad((table_0), ((_S92)).xy, ((_S92)).z))), decode_fixed_pair_0((textureLoad((table_0), ((_S94)).xy, ((_S94)).z))), _S95), mix(decode_fixed_pair_0((textureLoad((table_0), ((_S97)).xy, ((_S97)).z))), decode_fixed_pair_0((textureLoad((table_0), ((_S98)).xy, ((_S98)).z))), _S95), vec2<f32>(weight_3.y));
}

fn sky_prefilter_at_0( up_0 : f32,  roughness_0 : f32) -> vec2<f32>
{
    return fixed_pair_at_0(sky_prefilter_0, vec2<f32>(up_0, roughness_0));
}

fn sky_prefiltered_0( direction_5 : vec3<f32>,  roughness_1 : f32) -> vec3<f32>
{
    var up_1 : f32 = clamp(direction_5.y, -1.0f, 1.0f);
    var weights_0 : vec2<f32> = sky_prefilter_at_0(abs(up_1), roughness_1);
    var _S99 : bool = up_1 >= 0.0f;
    var far_0 : vec3<f32>;
    if(_S99)
    {
        far_0 = camera_0.sky_0[i32(0)].xyz;
    }
    else
    {
        far_0 = camera_0.sky_0[i32(2)].xyz;
    }
    var opposite_0 : vec3<f32>;
    if(_S99)
    {
        opposite_0 = camera_0.sky_0[i32(2)].xyz;
    }
    else
    {
        opposite_0 = camera_0.sky_0[i32(0)].xyz;
    }
    var _S100 : f32 = weights_0.x;
    var _S101 : f32 = weights_0.y;
    return camera_0.sky_0[i32(1)].xyz * vec3<f32>((1.0f - _S100 - _S101)) + far_0 * vec3<f32>(_S100) + opposite_0 * vec3<f32>(_S101);
}

fn sky_view_at_0( up_2 : f32,  azimuth_cosine_0 : f32) -> vec3<f32>
{
    var u_0 : f32 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    var clamped_1 : f32 = clamp(up_2, -1.0f, 1.0f);
    var root_0 : f32 = sqrt(abs(clamped_1));
    var _S102 : f32;
    if(clamped_1 >= 0.0f)
    {
        _S102 = root_0;
    }
    else
    {
        _S102 = - root_0;
    }
    var across_0 : f32 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    var x0_0 : f32 = clamp(floor(across_0), 0.0f, 95.0f);
    var fx_0 : f32 = clamp(across_0 - x0_0, 0.0f, 1.0f);
    var down_0 : f32 = clamp(0.5f + 0.5f * _S102, 0.0f, 1.0f) * 64.0f - 0.5f;
    var y0_0 : f32 = clamp(floor(down_0), 0.0f, 63.0f);
    var fy_0 : f32 = clamp(down_0 - y0_0, 0.0f, 1.0f);
    var row0_0 : u32 = u32(y0_0) * u32(96);
    var row1_0 : u32 = u32(min(y0_0 + 1.0f, 63.0f)) * u32(96);
    var _S103 : u32 = u32(x0_0);
    var _S104 : vec3<f32> = vec3<f32>((1.0f - fx_0));
    var _S105 : u32 = u32(min(x0_0 + 1.0f, 95.0f));
    var _S106 : vec3<f32> = vec3<f32>(fx_0);
    return (sky_view_0[row0_0 + _S103].xyz * _S104 + sky_view_0[row0_0 + _S105].xyz * _S106) * vec3<f32>((1.0f - fy_0)) + (sky_view_0[row1_0 + _S103].xyz * _S104 + sky_view_0[row1_0 + _S105].xyz * _S106) * vec3<f32>(fy_0);
}

fn atmosphere_radiance_0( direction_6 : vec3<f32>) -> vec3<f32>
{
    var sun_0 : vec3<f32> = camera_0.atmosphere_0.xyz;
    var _S107 : f32 = direction_6.x;
    var _S108 : f32 = direction_6.z;
    var view_flat_0 : f32 = sqrt(_S107 * _S107 + _S108 * _S108);
    var _S109 : f32 = sun_0.x;
    var _S110 : f32 = sun_0.z;
    var sun_flat_0 : f32 = sqrt(_S109 * _S109 + _S110 * _S110);
    var _S111 : bool;
    if(view_flat_0 > 0.0f)
    {
        _S111 = sun_flat_0 > 0.0f;
    }
    else
    {
        _S111 = false;
    }
    var cosine_1 : f32;
    if(_S111)
    {
        cosine_1 = (_S107 * _S109 + _S108 * _S110) / (view_flat_0 * sun_flat_0);
    }
    else
    {
        cosine_1 = 1.0f;
    }
    return sky_view_at_0(direction_6.y, cosine_1);
}

fn sky_environment_0( direction_7 : vec3<f32>,  roughness_2 : f32,  share_1 : f32) -> vec3<f32>
{
    var bands_0 : vec3<f32> = sky_prefiltered_0(direction_7, roughness_2);
    if((camera_0.atmosphere_0.w) <= 0.0f)
    {
        return bands_0;
    }
    return bands_0 * vec3<f32>((1.0f - share_1)) + atmosphere_radiance_0(direction_7) * vec3<f32>(share_1);
}

fn fog_one_minus_exp_over_0( d_0 : f32) -> f32
{
    if((abs(d_0)) < 0.125f)
    {
        var _S112 : f32 = - d_0;
        var series_0 : f32 = 0.00833333376795053f;
        var term_2 : i32 = i32(3);
        for(;;)
        {
            if(term_2 >= i32(0))
            {
            }
            else
            {
                break;
            }
            var _S113 : f32 = series_0 * _S112 + FOG_RATIO_KERNEL_0[term_2];
            var term_3 : i32 = term_2 - i32(1);
            series_0 = _S113;
            term_2 = term_3;
        }
        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}

fn fog_optical_depth_0( density_0 : f32,  falloff_0 : f32,  height_a_0 : f32,  height_b_0 : f32,  distance_0 : f32) -> f32
{
    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_0, 0.0f, 32.0f);
    }
    return clamp(density_0 * distance_0 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}

fn volumetric_phase_0( g_0 : f32,  cos_theta_0 : f32) -> f32
{
    var a_1 : f32 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    var _S114 : f32 = a_1 * a_1;
    var d_1 : f32 = 1.0f + _S114 - 2.0f * a_1 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S114) / (d_1 * sqrt(d_1));
}

fn volumetric_source_0( view_direction_0 : vec3<f32>,  lit_0 : vec4<f32>) -> vec3<f32>
{
    return params_0.fog_color_1.xyz + params_0.sun_radiance_0.xyz * vec3<f32>(volumetric_phase_0(params_0.sun_direction_0.w, dot(params_0.sun_direction_0.xyz, view_direction_0))) * vec3<f32>(lit_0.w) + lit_0.xyz;
}

struct WaterAir_0
{
     survives_0 : f32,
     inscatter_0 : vec3<f32>,
};

fn water_froxel_air_0( pixel_7 : vec2<i32>,  depth_18 : f32) -> WaterAir_0
{
    var air_0 : WaterAir_0;
    air_0.survives_0 = 1.0f;
    air_0.inscatter_0 = vec3<f32>(0.0f, 0.0f, 0.0f);
    var _S115 : u32 = max(params_0.grid_x_0, u32(1));
    var _S116 : u32 = max(params_0.grid_y_0, u32(1));
    var _S117 : u32 = max(params_0.slices_0, u32(1));
    var tiles_0 : u32 = _S115 * _S116;
    var _S118 : u32 = max(params_0.tile_pixels_0, u32(1));
    var _S119 : i32 = pixel_7.x;
    var _S120 : i32 = pixel_7.y;
    var ndc_2 : vec2<f32> = vec2<f32>((f32(_S119) + 0.5f) / f32(max(params_0.viewport_x_0, u32(1))) * 2.0f - 1.0f, 1.0f - (f32(_S120) + 0.5f) / f32(max(params_0.viewport_y_0, u32(1))) * 2.0f);
    var view_depth_0 : f32;
    if(depth_18 > 0.0f)
    {
        view_depth_0 = dot(params_0.depth_row_0, vec4<f32>(volumetric_unproject_0(ndc_2, depth_18), 1.0f));
    }
    else
    {
        view_depth_0 = 1000.0f;
    }
    var view_depth_1 : f32 = clamp(view_depth_0, 0.0f, 1000.0f);
    var slice_start_0 : f32 = 0.0f;
    var slice_0 : u32 = u32(0);
    var next_start_0 : f32 = 0.14677993953227997f;
    for(;;)
    {
        var _S121 : u32 = slice_0 + u32(1);
        var _S122 : bool;
        if(_S121 < _S117)
        {
            _S122 = next_start_0 <= view_depth_1;
        }
        else
        {
            _S122 = false;
        }
        if(_S122)
        {
        }
        else
        {
            break;
        }
        var next_start_1 : f32 = next_start_0 * 1.46779930591583252f;
        slice_start_0 = next_start_0;
        next_start_0 = next_start_1;
        slice_0 = _S121;
    }
    var _S123 : u32 = u32(max(_S119, i32(0))) / _S118;
    var _S124 : u32 = min(_S123, _S115 - u32(1));
    var _S125 : u32 = u32(max(_S120, i32(0))) / _S118;
    var froxel_0 : u32 = _S124 + min(_S125, _S116 - u32(1)) * _S115 + slice_0 * tiles_0;
    if(froxel_0 >= (params_0.froxel_count_0))
    {
        return air_0;
    }
    var prefix_0 : vec4<f32> = volumetrics_0[froxel_0];
    var near_point_0 : vec3<f32> = volumetric_unproject_0(ndc_2, 1.0f);
    var along_0 : vec3<f32> = (near_point_0 - params_0.eye_0.xyz) / vec3<f32>(max(dot(params_0.depth_row_0, vec4<f32>(near_point_0, 1.0f)), 9.99999997475242708e-07f));
    var from_0 : vec3<f32> = params_0.eye_0.xyz + along_0 * vec3<f32>(slice_start_0);
    var to_0 : vec3<f32> = params_0.eye_0.xyz + along_0 * vec3<f32>(max(view_depth_1, slice_start_0));
    var reference_4 : f32 = params_0.fog_params_1.z;
    var segment_0 : vec3<f32> = to_0 - from_0;
    var length_of_0 : f32 = length(segment_0);
    var partial_survives_0 : f32 = fog_exp_neg_0(fog_optical_depth_0(params_0.fog_params_1.x, params_0.fog_params_1.y, from_0.y - reference_4, to_0.y - reference_4, length_of_0));
    var view_direction_1 : vec3<f32>;
    if(length_of_0 > 9.99999997475242708e-07f)
    {
        view_direction_1 = segment_0 / vec3<f32>(length_of_0);
    }
    else
    {
        view_direction_1 = vec3<f32>(0.0f, 0.0f, 1.0f);
    }
    var partial_radiance_0 : vec3<f32> = volumetric_source_0(view_direction_1, lighting_0[froxel_0]) * vec3<f32>((1.0f - partial_survives_0));
    var _S126 : f32 = prefix_0.w;
    air_0.survives_0 = _S126 * partial_survives_0;
    air_0.inscatter_0 = prefix_0.xyz + vec3<f32>(_S126) * partial_radiance_0;
    return air_0;
}

fn fog_transmittance_0( optical_depth_0 : f32) -> f32
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}

fn water_analytic_air_0( surface_1 : vec3<f32>) -> WaterAir_0
{
    var fog_survives_0 : f32 = fog_transmittance_0(fog_optical_depth_0(frame_0.fog_params_0.x, frame_0.fog_params_0.y, frame_0.camera_position_0.y - frame_0.fog_params_0.z, surface_1.y - frame_0.fog_params_0.z, length(frame_0.camera_position_0.xyz - surface_1)));
    var air_1 : WaterAir_0;
    air_1.survives_0 = fog_survives_0;
    air_1.inscatter_0 = frame_0.fog_color_0.xyz * vec3<f32>((1.0f - fog_survives_0));
    return air_1;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) world_position_9 : vec3<f32>,
    @interpolate(flat) @location(1) body_2 : u32,
};

@fragment
fn fragmentMain( _S127 : pixelInput_0, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_0
{
    var width_2 : u32;
    var height_2 : u32;
    {var dim = textureDimensions((scene_depth_0));((width_2)) = dim.x;((height_2)) = dim.y;};
    var extent_3 : vec2<f32> = vec2<f32>(f32(width_2), f32(height_2));
    var last_2 : vec2<i32> = vec2<i32>(i32(width_2) - i32(1), i32(height_2) - i32(1));
    var _S128 : vec2<f32> = position_2.xy;
    var _S129 : vec2<i32> = vec2<i32>(_S128);
    var surface_depth_0 : f32 = position_2.z;
    var medium_0 : WaterMedium_std430_0 = media_0[_S127.body_2];
    const up_3 : vec3<f32> = vec3<f32>(0.0f, 1.0f, 0.0f);
    var incident_1 : vec3<f32> = normalize(_S127.world_position_9 - frame_0.camera_position_0.xyz);
    var _S130 : f32 = - incident_1.y;
    var cos_incident_1 : f32 = saturate(_S130);
    var _S131 : vec3<i32> = vec3<i32>(_S129, i32(0));
    var straight_0 : vec4<f32> = (textureLoad((scene_color_0), ((_S131)).xy, ((_S131)).z));
    var straight_point_0 : vec3<f32> = water_opaque_point_0(_S129, (textureLoad((scene_depth_0), ((_S131)).xy, ((_S131)).z)), extent_3, _S127.world_position_9, incident_1);
    var refracted_0 : vec3<f32> = water_refract_0(incident_1, up_3, 0.75f);
    var landing_clip_0 : vec4<f32> = (((vec4<f32>(_S127.world_position_9 + refracted_0 * vec3<f32>((max(_S127.world_position_9.y - straight_point_0.y, 0.0f) / max(- refracted_0.y, 9.99999997475242708e-07f))), 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_0[i32(0)][i32(0)], frame_0.view_proj_0.data_0[i32(1)][i32(0)], frame_0.view_proj_0.data_0[i32(2)][i32(0)], frame_0.view_proj_0.data_0[i32(3)][i32(0)], frame_0.view_proj_0.data_0[i32(0)][i32(1)], frame_0.view_proj_0.data_0[i32(1)][i32(1)], frame_0.view_proj_0.data_0[i32(2)][i32(1)], frame_0.view_proj_0.data_0[i32(3)][i32(1)], frame_0.view_proj_0.data_0[i32(0)][i32(2)], frame_0.view_proj_0.data_0[i32(1)][i32(2)], frame_0.view_proj_0.data_0[i32(2)][i32(2)], frame_0.view_proj_0.data_0[i32(3)][i32(2)], frame_0.view_proj_0.data_0[i32(0)][i32(3)], frame_0.view_proj_0.data_0[i32(1)][i32(3)], frame_0.view_proj_0.data_0[i32(2)][i32(3)], frame_0.view_proj_0.data_0[i32(3)][i32(3)]))));
    var _S132 : f32 = landing_clip_0.w;
    var behind_point_0 : vec3<f32>;
    var behind_1 : vec4<f32>;
    if(_S132 > 0.0f)
    {
        var landing_ndc_0 : vec2<f32> = landing_clip_0.xy / vec2<f32>(_S132);
        var bent_0 : vec2<i32> = clamp(vec2<i32>(vec2<f32>(landing_ndc_0.x * 0.5f + 0.5f, 0.5f - landing_ndc_0.y * 0.5f) * extent_3), vec2<i32>(i32(0), i32(0)), last_2);
        var _S133 : vec3<i32> = vec3<i32>(bent_0, i32(0));
        var bent_depth_0 : f32 = (textureLoad((scene_depth_0), ((_S133)).xy, ((_S133)).z));
        if(bent_depth_0 < surface_depth_0)
        {
            var _S134 : vec4<f32> = (textureLoad((scene_color_0), ((_S133)).xy, ((_S133)).z));
            behind_point_0 = water_opaque_point_0(bent_0, bent_depth_0, extent_3, _S127.world_position_9, refracted_0);
            behind_1 = _S134;
        }
        else
        {
            behind_point_0 = straight_point_0;
            behind_1 = straight_0;
        }
    }
    else
    {
        behind_point_0 = straight_point_0;
        behind_1 = straight_0;
    }
    var thickness_0 : f32 = length(behind_point_0 - _S127.world_position_9);
    var extinction_0 : vec3<f32> = medium_0.absorption_0.xyz + medium_0.scattering_0.xyz;
    var _S135 : f32 = extinction_0.x;
    var _S136 : f32 = extinction_0.y;
    var _S137 : f32 = extinction_0.z;
    var transmittance_0 : vec3<f32> = vec3<f32>(fog_exp_neg_0(_S135 * thickness_0), fog_exp_neg_0(_S136 * thickness_0), fog_exp_neg_0(_S137 * thickness_0));
    var _S138 : f32;
    if(_S135 > 0.0f)
    {
        _S138 = medium_0.scattering_0.x / _S135;
    }
    else
    {
        _S138 = 0.0f;
    }
    var _S139 : f32;
    if(_S136 > 0.0f)
    {
        _S139 = medium_0.scattering_0.y / _S136;
    }
    else
    {
        _S139 = 0.0f;
    }
    var _S140 : f32;
    if(_S137 > 0.0f)
    {
        _S140 = medium_0.scattering_0.z / _S137;
    }
    else
    {
        _S140 = 0.0f;
    }
    var to_sun_0 : vec3<f32> = water_0.sun_direction_1.xyz;
    var _S141 : f32 = to_sun_0.y;
    var cascade_3 : u32;
    var cascade_fade_0 : f32;
    var _S142 : vec3<f32> = vec3<f32>(1.0f);
    var scattered_0 : vec3<f32> = vec3<f32>(_S138, _S139, _S140) * (frame_0.ambient_0.xyz + sky_irradiance_0(up_3) + water_0.sun_color_0.xyz * vec3<f32>((max(_S141, 0.0f) * sun_visibility_0(_S127.world_position_9, to_sun_0, _S141, up_3, _S128, &(cascade_3), &(cascade_fade_0))))) * (_S142 - transmittance_0);
    var mirrored_0 : vec3<f32> = vec3<f32>(incident_1.x, _S130, incident_1.z);
    var environment_0 : vec3<f32> = probe_environment_0(_S127.world_position_9, up_3, mirrored_0) + sky_environment_0(mirrored_0, 0.0f, 1.0f);
    var grazing_0 : f32 = 1.0f - cos_incident_1;
    var grazing2_0 : f32 = grazing_0 * grazing_0;
    var fresnel_0 : f32 = 0.01999999955296516f + 0.98000001907348633f * (grazing2_0 * grazing2_0 * grazing_0);
    var air_2 : WaterAir_0;
    if((water_0.froxels_0.x) != u32(0))
    {
        var _S143 : WaterAir_0 = water_froxel_air_0(_S129, surface_depth_0);
        air_2 = _S143;
    }
    else
    {
        air_2 = water_analytic_air_0(_S127.world_position_9);
    }
    var _S144 : vec3<f32> = vec3<f32>((1.0f - fresnel_0));
    var fade_1 : f32 = saturate(thickness_0 / 0.25f);
    var _S145 : pixelOutput_0 = pixelOutput_0( vec4<f32>(straight_0.xyz * vec3<f32>((1.0f - fade_1)) + (behind_1.xyz * transmittance_0 * _S144 + (environment_0 * vec3<f32>(fresnel_0) + scattered_0 * _S144) * vec3<f32>(air_2.survives_0) + air_2.inscatter_0 * (_S142 - transmittance_0 * _S144)) * vec3<f32>(fade_1), straight_0.w) );
    return _S145;
}

