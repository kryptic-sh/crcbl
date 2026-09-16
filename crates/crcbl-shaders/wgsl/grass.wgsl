struct GrassTile_std140_0
{
    @align(16) tile_0 : vec4<f32>,
    @align(16) slot_0 : vec4<u32>,
};

@binding(2) @group(0) var<uniform> tile_1 : GrassTile_std140_0;
struct GrassParams_std140_0
{
    @align(16) limits_0 : vec4<u32>,
    @align(16) screen_0 : vec4<f32>,
};

@binding(1) @group(0) var<uniform> grass_0 : GrassParams_std140_0;
struct GrassInstance_std430_0
{
    @align(16) root_0 : vec4<f32>,
    @align(16) facing_0 : vec4<f32>,
    @align(16) lean_0 : vec4<f32>,
    @align(16) ground_0 : vec4<f32>,
    @align(16) lanes_0 : vec4<u32>,
};

@binding(3) @group(0) var<storage, read> instances_0 : array<GrassInstance_std430_0>;

struct GrassBlade_std430_0
{
    @align(16) root_color_0 : vec4<f32>,
    @align(16) tip_color_0 : vec4<f32>,
    @align(16) size_0 : vec4<f32>,
};

@binding(4) @group(0) var<storage, read> blades_0 : array<GrassBlade_std430_0>;

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

@binding(0) @group(0) var<uniform> frame_0 : FrameUniforms_std140_0;
@binding(5) @group(0) var grassCard_0 : texture_2d_array<f32>;

@binding(6) @group(0) var grassCardSampler_0 : sampler;

@binding(10) @group(0) var<storage, read> cluster_lights_0 : array<u32>;

struct GpuLight_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
    @align(16) direction_0 : vec4<f32>,
    @align(16) tangent_0 : vec4<f32>,
    @align(16) kind_0 : u32,
    @align(4) cos_inner_0 : f32,
    @align(8) shadow_tile_0 : u32,
    @align(4) flags_0 : u32,
};

@binding(9) @group(0) var<storage, read> lights_0 : array<GpuLight_std430_0>;

@binding(7) @group(0) var shadow_atlas_0 : texture_depth_2d;

@binding(8) @group(0) var shadow_sampler_0 : sampler_comparison;

var<private> SHADOW_SEARCH_DISC_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(0.17677700519561768f, 0.0f), vec2<f32>(-0.22577199339866638f, 0.20682600140571594f), vec2<f32>(0.0345579981803894f, -0.39377099275588989f), vec2<f32>(0.28457099199295044f, 0.37117299437522888f), vec2<f32>(-0.52222299575805664f, -0.09237399697303772f), vec2<f32>(0.49469500780105591f, -0.31468498706817627f), vec2<f32>(-0.16546599566936493f, 0.6155250072479248f), vec2<f32>(-0.31556099653244019f, -0.60759401321411133f), vec2<f32>(0.68464201688766479f, 0.25003001093864441f), vec2<f32>(-0.71225601434707642f, 0.2940090000629425f), vec2<f32>(0.3433539867401123f, -0.73372900485992432f), vec2<f32>(0.25372999906539917f, 0.80893200635910034f), vec2<f32>(-0.76474601030349731f, -0.44318601489067078f), vec2<f32>(0.89713400602340698f, -0.19723199307918549f), vec2<f32>(-0.54750698804855347f, 0.77877199649810791f), vec2<f32>(-0.12648700177669525f, -0.97609001398086548f) );
var<private> SHADOW_DISC_0 : array<vec2<f32>, i32(32)> = array<vec2<f32>, i32(32)>( vec2<f32>(0.125f, 0.0f), vec2<f32>(-0.15964500606060028f, 0.14624799787998199f), vec2<f32>(0.02443600073456764f, -0.27843800187110901f), vec2<f32>(0.2012220025062561f, 0.26245900988578796f), vec2<f32>(-0.36926800012588501f, -0.06531800329685211f), vec2<f32>(0.34980198740959167f, -0.22251600027084351f), vec2<f32>(-0.11700200289487839f, 0.43524199724197388f), vec2<f32>(-0.22313599288463593f, -0.42963400483131409f), vec2<f32>(0.48411500453948975f, 0.17679800093173981f), vec2<f32>(-0.50364100933074951f, 0.20789599418640137f), vec2<f32>(0.24278800189495087f, -0.51882398128509521f), vec2<f32>(0.17941400408744812f, 0.57200098037719727f), vec2<f32>(-0.54075700044631958f, -0.31338000297546387f), vec2<f32>(0.63437002897262573f, -0.13946400582790375f), vec2<f32>(-0.38714599609375f, 0.55067497491836548f), vec2<f32>(-0.0894400030374527f, -0.69019997119903564f), vec2<f32>(0.5490720272064209f, 0.46275800466537476f), vec2<f32>(-0.73887801170349121f, 0.0305550005286932f), vec2<f32>(0.5389549732208252f, -0.53633201122283936f), vec2<f32>(-0.03605800122022629f, 0.77979201078414917f), vec2<f32>(-0.51281797885894775f, -0.61452698707580566f), vec2<f32>(0.81235998868942261f, 0.10930199921131134f), vec2<f32>(-0.68831098079681396f, 0.47890898585319519f), vec2<f32>(0.18808600306510925f, -0.83606100082397461f), vec2<f32>(0.43503299355506897f, 0.75919097661972046f), vec2<f32>(-0.85044801235198975f, -0.27131599187850952f), vec2<f32>(0.82610201835632324f, -0.38168001174926758f), vec2<f32>(-0.35788801312446594f, 0.85515600442886353f), vec2<f32>(-0.31940698623657227f, -0.88803398609161377f), vec2<f32>(0.84990900754928589f, 0.44668799638748169f), vec2<f32>(-0.94403499364852905f, 0.24884499609470367f), vec2<f32>(0.53659600019454956f, -0.83452999591827393f) );
var<private> SHADOW_PROBE_INDEX_0 : array<u32, i32(5)> = array<u32, i32(5)>( u32(0), u32(23), u32(25), u32(27), u32(29) );
var<private> SHADOW_ROTATIONS_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(1.0f, 0.0f), vec2<f32>(0.92387998104095459f, 0.38268300890922546f), vec2<f32>(0.70710700750350952f, 0.70710700750350952f), vec2<f32>(0.38268300890922546f, 0.92387998104095459f), vec2<f32>(0.0f, 1.0f), vec2<f32>(-0.38268300890922546f, 0.92387998104095459f), vec2<f32>(-0.70710700750350952f, 0.70710700750350952f), vec2<f32>(-0.92387998104095459f, 0.38268300890922546f), vec2<f32>(-1.0f, 0.0f), vec2<f32>(-0.92387998104095459f, -0.38268300890922546f), vec2<f32>(-0.70710700750350952f, -0.70710700750350952f), vec2<f32>(-0.38268300890922546f, -0.92387998104095459f), vec2<f32>(-0.0f, -1.0f), vec2<f32>(0.38268300890922546f, -0.92387998104095459f), vec2<f32>(0.70710700750350952f, -0.70710700750350952f), vec2<f32>(0.92387998104095459f, -0.38268300890922546f) );
var<private> SHADOW_DITHER_0 : array<u32, i32(16)> = array<u32, i32(16)>( u32(0), u32(8), u32(2), u32(10), u32(12), u32(4), u32(14), u32(6), u32(3), u32(11), u32(1), u32(9), u32(15), u32(7), u32(13), u32(5) );
var<private> GRASS_CARD_CORNERS_0 : array<vec2<f32>, i32(6)> = array<vec2<f32>, i32(6)>( vec2<f32>(0.0f, 0.0f), vec2<f32>(1.0f, 0.0f), vec2<f32>(0.0f, 1.0f), vec2<f32>(1.0f, 0.0f), vec2<f32>(1.0f, 1.0f), vec2<f32>(0.0f, 1.0f) );
fn grassCardLevel_0( pixels_0 : f32) -> f32
{
    var _S1 : f32 = 64.0f / max(pixels_0, 0.00009999999747379f);
    var level_0 : f32 = 0.0f;
    var step_0 : u32 = u32(1);
    for(;;)
    {
        if(step_0 < u32(7))
        {
        }
        else
        {
            break;
        }
        var _S2 : u32 = step_0 - u32(1);
        var lower_0 : f32 = f32((u32(1) << (_S2)));
        if(_S1 >= lower_0)
        {
            level_0 = f32(_S2) + min((_S1 - lower_0) / lower_0, 1.0f);
        }
        step_0 = step_0 + u32(1);
    }
    return min(floor(level_0 + 0.5f), 6.0f);
}

struct GrassVertex_0
{
    @builtin(position) position_1 : vec4<f32>,
    @location(0) world_position_0 : vec3<f32>,
    @location(1) normal_0 : vec3<f32>,
    @location(2) color_1 : vec3<f32>,
    @location(3) uv_0 : vec2<f32>,
    @location(4) level_1 : f32,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32, @builtin(instance_index) instance_id_0 : u32) -> GrassVertex_0
{
    var blade_0 : GrassInstance_std430_0 = instances_0[tile_1.slot_0.x * grass_0.limits_0.x + instance_id_0];
    var row_0 : GrassBlade_std430_0 = blades_0[min(blade_0.lanes_0.y, max(grass_0.limits_0.y, u32(1)) - u32(1))];
    var _S3 : u32 = index_0 % u32(6);
    var facing_1 : vec2<f32> = blade_0.facing_0.xy;
    var across_0 : vec2<f32>;
    if((index_0 / u32(6)) == u32(0))
    {
        across_0 = facing_1;
    }
    else
    {
        across_0 = vec2<f32>(- facing_1.y, facing_1.x);
    }
    var _S4 : f32 = blade_0.facing_0.z;
    var _S5 : f32 = GRASS_CARD_CORNERS_0[_S3].y;
    var world_0 : vec3<f32> = blade_0.root_0.xyz + vec3<f32>(across_0.x, 0.0f, across_0.y) * vec3<f32>((_S4 * (GRASS_CARD_CORNERS_0[_S3].x * 2.0f - 1.0f))) + vec3<f32>(0.0f, blade_0.root_0.w * _S5, 0.0f) + blade_0.lean_0.xyz * vec3<f32>((_S5 * _S5));
    var output_0 : GrassVertex_0;
    var _S6 : vec4<f32> = (((vec4<f32>(world_0, 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_0[i32(0)][i32(0)], frame_0.view_proj_0.data_0[i32(1)][i32(0)], frame_0.view_proj_0.data_0[i32(2)][i32(0)], frame_0.view_proj_0.data_0[i32(3)][i32(0)], frame_0.view_proj_0.data_0[i32(0)][i32(1)], frame_0.view_proj_0.data_0[i32(1)][i32(1)], frame_0.view_proj_0.data_0[i32(2)][i32(1)], frame_0.view_proj_0.data_0[i32(3)][i32(1)], frame_0.view_proj_0.data_0[i32(0)][i32(2)], frame_0.view_proj_0.data_0[i32(1)][i32(2)], frame_0.view_proj_0.data_0[i32(2)][i32(2)], frame_0.view_proj_0.data_0[i32(3)][i32(2)], frame_0.view_proj_0.data_0[i32(0)][i32(3)], frame_0.view_proj_0.data_0[i32(1)][i32(3)], frame_0.view_proj_0.data_0[i32(2)][i32(3)], frame_0.view_proj_0.data_0[i32(3)][i32(3)]))));
    output_0.position_1 = _S6;
    output_0.world_position_0 = world_0;
    output_0.normal_0 = blade_0.ground_0.xyz;
    output_0.color_1 = mix(row_0.root_color_0.xyz, row_0.tip_color_0.xyz, vec3<f32>(_S5)) * vec3<f32>((1.0f - 0.18000000715255737f * blade_0.facing_0.w));
    output_0.uv_0 = GRASS_CARD_CORNERS_0[_S3];
    output_0.level_1 = grassCardLevel_0(2.0f * _S4 * grass_0.screen_0.x / max(abs(_S6.w), 0.00009999999747379f));
    return output_0;
}

fn froxel_of_0( pixel_0 : vec2<f32>,  depth_0 : f32) -> u32
{
    var _S7 : u32 = max(frame_0.cluster_grid_0.x, u32(1));
    var _S8 : u32 = max(frame_0.cluster_grid_0.y, u32(1));
    var _S9 : u32 = max(frame_0.cluster_grid_0.z, u32(1));
    var _S10 : u32 = max(frame_0.cluster_grid_0.w, u32(1));
    var _S11 : u32 = u32(pixel_0.x) / _S10;
    var _S12 : u32 = min(_S11, _S7 - u32(1));
    var _S13 : u32 = u32(pixel_0.y) / _S10;
    var scale_0 : f32 = 24.0f / log2(10000.0f);
    return (u32(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, f32(_S9 - u32(1)))) * _S8 + min(_S13, _S8 - u32(1))) * _S7 + _S12;
}

fn range_window_0( distance_0 : f32,  radius_0 : f32) -> f32
{
    var ratio_0 : f32 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    var window_0 : f32 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}

fn punctual_falloff_0( distance_1 : f32,  radius_1 : f32) -> f32
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}

fn spot_cone_0( to_light_0 : vec3<f32>,  axis_0 : vec3<f32>,  cos_outer_0 : f32,  cos_inner_1 : f32) -> f32
{
    return saturate((dot((vec3<f32>(0) - to_light_0), normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}

fn atlas_rect_0( tile_2 : u32) -> vec4<f32>
{
    return frame_0.shadow_atlas_rect_0[tile_2];
}

fn atlas_rect_is_empty_0( rect_0 : vec4<f32>) -> bool
{
    return !((rect_0.x) > 0.0f);
}

fn tile_texels_0( rect_1 : vec4<f32>) -> f32
{
    return rect_1.x / frame_0.shadow_params_0.x;
}

fn shadow_normal_offset_0( geometric_normal_0 : vec3<f32>,  to_light_1 : vec3<f32>) -> f32
{
    var cosine_0 : f32 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}

fn shadow_filter_mode_0( pixel_1 : vec2<f32>) -> u32
{
    var _S14 : u32;
    if(u32(pixel_1.x) < (frame_0.shadow_filter_0.z))
    {
        _S14 = frame_0.shadow_filter_0.x;
    }
    else
    {
        _S14 = frame_0.shadow_filter_0.y;
    }
    return _S14;
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
    var _S15 : f32 = spoke_0.x;
    var _S16 : f32 = rotation_0.x;
    var _S17 : f32 = spoke_0.y;
    var _S18 : f32 = rotation_0.y;
    return (textureSampleCompareLevel((shadow_atlas_0), (shadow_sampler_0), (atlas_uv_0(rect_4, clamp(tile_uv_1 + vec2<f32>(_S15 * _S16 - _S17 * _S18, _S15 * _S18 + _S17 * _S16) * texel_step_0, tile_min_0, vec2<f32>(1.0f) - tile_min_0))), (reference_0)));
}

fn tile_box_pcf_0( tile_3 : u32,  tile_uv_2 : vec2<f32>,  reference_1 : f32) -> f32
{
    var rect_5 : vec4<f32> = atlas_rect_0(tile_3);
    if(atlas_rect_is_empty_0(rect_5))
    {
        return 1.0f;
    }
    var _S19 : vec2<f32> = atlas_step_0(rect_5);
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
        var x_0 : i32 = i32(-1);
        for(;;)
        {
            if(x_0 <= i32(1))
            {
            }
            else
            {
                break;
            }
            var visibility_1 : f32 = visibility_0 + tile_tap_0(rect_5, _S19, tile_uv_2, vec2<f32>(f32(x_0), f32(y_0)), vec2<f32>(1.0f, 0.0f), reference_1);
            x_0 = x_0 + i32(1);
            visibility_0 = visibility_1;
        }
        y_0 = y_0 + i32(1);
    }
    return visibility_0 / 9.0f;
}

fn shadow_rotation_0( pixel_2 : vec2<f32>) -> vec2<f32>
{
    var cell_0 : vec2<u32> = (vec2<u32>(pixel_2) & (vec2<u32>(u32(3))));
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * u32(4) + cell_0.x]];
}

fn tile_pcf_0( tile_4 : u32,  tile_uv_3 : vec2<f32>,  reference_2 : f32,  pixel_3 : vec2<f32>,  radius_2 : f32) -> f32
{
    var _S20 : vec2<f32> = shadow_rotation_0(pixel_3);
    var rect_6 : vec4<f32> = atlas_rect_0(tile_4);
    if(atlas_rect_is_empty_0(rect_6))
    {
        return 1.0f;
    }
    var _S21 : vec2<f32> = atlas_step_0(rect_6);
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
        var probe_1 : f32 = probe_0 + tile_tap_0(rect_6, _S21, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * vec2<f32>(radius_2), _S20, reference_2);
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
    var index_1 : u32 = u32(0);
    var visibility_2 : f32 = 0.0f;
    for(;;)
    {
        if(index_1 < u32(32))
        {
        }
        else
        {
            break;
        }
        var visibility_3 : f32 = visibility_2 + tile_tap_0(rect_6, _S21, tile_uv_3, SHADOW_DISC_0[index_1] * vec2<f32>(radius_2), _S20, reference_2);
        index_1 = index_1 + u32(1);
        visibility_2 = visibility_3;
    }
    return visibility_2 / 32.0f;
}

fn sun_penumbra_texels_0( cascade_0 : u32,  tile_uv_4 : vec2<f32>,  reference_3 : f32,  rotation_1 : vec2<f32>) -> f32
{
    var rect_7 : vec4<f32> = atlas_rect_0(cascade_0);
    var texel_step_1 : vec2<f32> = atlas_step_0(rect_7);
    var _S22 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_step_1;
    const _S23 : vec2<f32> = vec2<f32>(1.0f, 1.0f);
    var _S24 : vec2<f32> = _S23 / frame_0.shadow_params_0.xy;
    var index_2 : u32 = u32(0);
    var sum_0 : f32 = 0.0f;
    var found_0 : f32 = 0.0f;
    for(;;)
    {
        if(index_2 < u32(16))
        {
        }
        else
        {
            break;
        }
        var spoke_1 : vec2<f32> = SHADOW_SEARCH_DISC_0[index_2] * vec2<f32>(8.0f);
        var _S25 : f32 = spoke_1.x;
        var _S26 : f32 = rotation_1.x;
        var _S27 : f32 = spoke_1.y;
        var _S28 : f32 = rotation_1.y;
        var _S29 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(_S25 * _S26 - _S27 * _S28, _S25 * _S28 + _S27 * _S26) * texel_step_1, _S22, vec2<f32>(1.0f) - _S22)) * _S24, _S24 - _S23)), i32(0));
        var depth_1 : f32 = (textureLoad((shadow_atlas_0), ((_S29)).xy, ((_S29)).z));
        if(depth_1 > reference_3)
        {
            var found_1 : f32 = found_0 + 1.0f;
            sum_0 = sum_0 + depth_1;
            found_0 = found_1;
        }
        index_2 = index_2 + u32(1);
    }
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }
    var _S30 : f32 = 2.0f * frame_0.cascade_far_0[cascade_0];
    return clamp((sum_0 / found_0 - reference_3) * (_S30 + 40.0f) * 0.01999999955296516f / (_S30 / tile_texels_0(rect_7)), 2.0f, 8.0f);
}

fn cascade_visibility_0( cascade_1 : u32,  world_position_1 : vec3<f32>,  to_light_2 : vec3<f32>,  geometric_normal_1 : vec3<f32>,  pixel_4 : vec2<f32>) -> f32
{
    var rect_8 : vec4<f32> = atlas_rect_0(cascade_1);
    if(atlas_rect_is_empty_0(rect_8))
    {
        return 1.0f;
    }
    var texel_world_0 : f32 = 2.0f * frame_0.cascade_far_0[cascade_1] / tile_texels_0(rect_8);
    var clip_0 : vec4<f32> = (((vec4<f32>(world_position_1 + geometric_normal_1 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2))) + to_light_2 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.z)), 1.0f)) * (mat4x4<f32>(frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(3)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(3)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(3)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(3)]))));
    var ndc_0 : vec3<f32> = clip_0.xyz / vec3<f32>(clip_0.w);
    var _S31 : bool;
    if((any(((abs(ndc_0.xy)) > vec2<f32>(1.0f)))))
    {
        _S31 = true;
    }
    else
    {
        _S31 = (ndc_0.z) <= 0.0f;
    }
    if(_S31)
    {
        return 1.0f;
    }
    var tile_uv_5 : vec2<f32> = vec2<f32>(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);
    var mode_0 : u32 = shadow_filter_mode_0(pixel_4);
    if(mode_0 == u32(2))
    {
        return tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z);
    }
    if(mode_0 == u32(1))
    {
        return tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f);
    }
    var _S32 : f32 = ndc_0.z;
    return tile_pcf_0(cascade_1, tile_uv_5, _S32, pixel_4, sun_penumbra_texels_0(cascade_1, tile_uv_5, _S32, shadow_rotation_0(pixel_4)));
}

fn sun_visibility_0( world_position_2 : vec3<f32>,  to_light_3 : vec3<f32>,  n_dot_l_0 : f32,  geometric_normal_2 : vec3<f32>,  pixel_5 : vec2<f32>,  selected_0 : ptr<function, u32>,  fade_0 : ptr<function, f32>) -> f32
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
    var index_3 : u32 = u32(0);
    for(;;)
    {
        if(index_3 < u32(2))
        {
        }
        else
        {
            covered_0 = false;
            cascade_2 = u32(1);
            break;
        }
        if(eye_distance_0 < (frame_0.cascade_far_0[index_3]))
        {
            covered_0 = true;
            cascade_2 = index_3;
            break;
        }
        index_3 = index_3 + u32(1);
    }
    if(covered_0)
    {
        (*selected_0) = cascade_2;
    }
    var visibility_4 : f32 = cascade_visibility_0(cascade_2, world_position_2, to_light_3, geometric_normal_2, pixel_5);
    var _S33 : u32 = cascade_2 + u32(1);
    if(_S33 >= u32(2))
    {
        return visibility_4;
    }
    var band_0 : f32 = frame_0.cascade_far_0[cascade_2] * 0.10000000149011612f;
    var blend_0 : f32 = saturate((eye_distance_0 - (frame_0.cascade_far_0[cascade_2] - band_0)) / band_0);
    (*fade_0) = blend_0;
    if(blend_0 <= 0.0f)
    {
        return visibility_4;
    }
    return mix(visibility_4, cascade_visibility_0(_S33, world_position_2, to_light_3, geometric_normal_2, pixel_5), blend_0);
}

fn point_face_0( from_light_0 : vec3<f32>) -> u32
{
    var axis_1 : vec3<f32> = abs(from_light_0);
    var _S34 : f32 = axis_1.x;
    var _S35 : f32 = axis_1.y;
    var _S36 : bool;
    if(_S34 >= _S35)
    {
        _S36 = _S34 >= (axis_1.z);
    }
    else
    {
        _S36 = false;
    }
    var _S37 : u32;
    if(_S36)
    {
        if((from_light_0.x) >= 0.0f)
        {
            _S37 = u32(0);
        }
        else
        {
            _S37 = u32(1);
        }
        return _S37;
    }
    if(_S35 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {
            _S37 = u32(2);
        }
        else
        {
            _S37 = u32(3);
        }
        return _S37;
    }
    if((from_light_0.z) >= 0.0f)
    {
        _S37 = u32(4);
    }
    else
    {
        _S37 = u32(5);
    }
    return _S37;
}

fn light_tile_0( tile_5 : u32) -> u32
{
    return u32(2) + tile_5;
}

fn punctual_visibility_0( tile_6 : u32,  world_position_3 : vec3<f32>,  to_light_4 : vec3<f32>,  n_dot_l_1 : f32,  map_world_0 : f32,  geometric_normal_3 : vec3<f32>,  pixel_6 : vec2<f32>) -> f32
{
    var atlas_0 : u32 = light_tile_0(tile_6);
    var rect_9 : vec4<f32> = atlas_rect_0(atlas_0);
    if(atlas_rect_is_empty_0(rect_9))
    {
        return 1.0f;
    }
    var texel_world_1 : f32 = map_world_0 / tile_texels_0(rect_9);
    var clip_1 : vec4<f32> = (((vec4<f32>(world_position_3 + geometric_normal_3 * vec3<f32>((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4))) + to_light_4 * vec3<f32>((texel_world_1 * 2.0f)), 1.0f)) * (mat4x4<f32>(frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(0)][i32(0)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(1)][i32(0)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(2)][i32(0)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(3)][i32(0)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(0)][i32(1)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(1)][i32(1)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(2)][i32(1)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(3)][i32(1)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(0)][i32(2)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(1)][i32(2)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(2)][i32(2)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(3)][i32(2)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(0)][i32(3)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(1)][i32(3)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(2)][i32(3)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(3)][i32(3)]))));
    var _S38 : f32 = clip_1.w;
    if(_S38 <= 0.0f)
    {
        return 1.0f;
    }
    var ndc_1 : vec3<f32> = clip_1.xyz / vec3<f32>(_S38);
    var _S39 : bool;
    if((any(((abs(ndc_1.xy)) > vec2<f32>(1.0f)))))
    {
        _S39 = true;
    }
    else
    {
        _S39 = (ndc_1.z) <= 0.0f;
    }
    if(_S39)
    {
        _S39 = true;
    }
    else
    {
        _S39 = (ndc_1.z) > 1.0f;
    }
    if(_S39)
    {
        return 1.0f;
    }
    var tile_uv_6 : vec2<f32> = vec2<f32>(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);
    if((shadow_filter_mode_0(pixel_6)) == u32(2))
    {
        return tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z);
    }
    return tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f);
}

fn point_visibility_0( light_0 : ptr<function, GpuLight_std430_0>,  base_0 : u32,  world_position_4 : vec3<f32>,  to_light_5 : vec3<f32>,  n_dot_l_2 : f32,  geometric_normal_4 : vec3<f32>,  pixel_7 : vec2<f32>) -> f32
{
    if(n_dot_l_2 <= 0.0f)
    {
        return 1.0f;
    }
    var from_light_1 : vec3<f32> = world_position_4 - (*light_0).position_0.xyz;
    return punctual_visibility_0(base_0 + point_face_0(from_light_1), world_position_4, to_light_5, n_dot_l_2, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7);
}

fn spot_visibility_0( light_1 : ptr<function, GpuLight_std430_0>,  tile_7 : u32,  world_position_5 : vec3<f32>,  to_light_6 : vec3<f32>,  n_dot_l_3 : f32,  geometric_normal_5 : vec3<f32>,  pixel_8 : vec2<f32>) -> f32
{
    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }
    var cos_outer_1 : f32 = (*light_1).direction_0.w;
    return punctual_visibility_0(tile_7, world_position_5, to_light_6, n_dot_l_3, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_5 - (*light_1).position_0.xyz, normalize((*light_1).direction_0.xyz)), 0.0f), geometric_normal_5, pixel_8);
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) world_position_6 : vec3<f32>,
    @location(1) normal_1 : vec3<f32>,
    @location(2) color_2 : vec3<f32>,
    @location(3) uv_1 : vec2<f32>,
    @location(4) level_2 : f32,
};

@fragment
fn fragmentMain( _S40 : pixelInput_0, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_0
{
    var _S41 : vec3<f32> = vec3<f32>(_S40.uv_1.x, 1.0f - _S40.uv_1.y, 0.0f);
    if(((textureSampleLevel((grassCard_0), (grassCardSampler_0), ((_S41)).xy, i32(((_S41)).z), (_S40.level_2))).x) < 0.5f)
    {
        discard;
    }
    var normal_2 : vec3<f32> = normalize(_S40.normal_1);
    var _S42 : vec2<f32> = position_2.xy;
    var _S43 : u32 = froxel_of_0(_S42, (((vec4<f32>(_S40.world_position_6, 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_0[i32(0)][i32(0)], frame_0.view_proj_0.data_0[i32(1)][i32(0)], frame_0.view_proj_0.data_0[i32(2)][i32(0)], frame_0.view_proj_0.data_0[i32(3)][i32(0)], frame_0.view_proj_0.data_0[i32(0)][i32(1)], frame_0.view_proj_0.data_0[i32(1)][i32(1)], frame_0.view_proj_0.data_0[i32(2)][i32(1)], frame_0.view_proj_0.data_0[i32(3)][i32(1)], frame_0.view_proj_0.data_0[i32(0)][i32(2)], frame_0.view_proj_0.data_0[i32(1)][i32(2)], frame_0.view_proj_0.data_0[i32(2)][i32(2)], frame_0.view_proj_0.data_0[i32(3)][i32(2)], frame_0.view_proj_0.data_0[i32(0)][i32(3)], frame_0.view_proj_0.data_0[i32(1)][i32(3)], frame_0.view_proj_0.data_0[i32(2)][i32(3)], frame_0.view_proj_0.data_0[i32(3)][i32(3)])))).w);
    var base_1 : u32 = _S43 * u32(17);
    var _S44 : u32 = min(cluster_lights_0[base_1], u32(16));
    const _S45 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var slot_1 : u32 = u32(0);
    var direct_0 : vec3<f32> = _S45;
    for(;;)
    {
        if(slot_1 < _S44)
        {
        }
        else
        {
            break;
        }
        var _S46 : GpuLight_std430_0 = lights_0[cluster_lights_0[base_1 + u32(1) + slot_1]];
        var _S47 : u32 = _S46.kind_0;
        if((_S46.kind_0) == u32(3))
        {
            slot_1 = slot_1 + u32(1);
            continue;
        }
        var _S48 : bool = _S47 == u32(0);
        var to_light_7 : vec3<f32>;
        var reach_0 : f32;
        if(_S48)
        {
            to_light_7 = normalize(_S46.direction_0.xyz);
            reach_0 = 1.0f;
        }
        else
        {
            var offset_0 : vec3<f32> = _S46.position_0.xyz - _S40.world_position_6;
            var distance_2 : f32 = length(offset_0);
            var to_light_8 : vec3<f32> = offset_0 / vec3<f32>(max(distance_2, 9.99999997475242708e-07f));
            var reach_1 : f32 = punctual_falloff_0(distance_2, _S46.position_0.w);
            if(_S47 == u32(2))
            {
                reach_0 = reach_1 * spot_cone_0(to_light_8, _S46.direction_0.xyz, _S46.direction_0.w, _S46.cos_inner_0);
            }
            else
            {
                reach_0 = reach_1;
            }
            to_light_7 = to_light_8;
        }
        var n_dot_l_4 : f32 = dot(normal_2, to_light_7);
        var reach_2 : f32;
        if(_S48)
        {
            var sun_cascade_0 : u32;
            var sun_fade_0 : f32;
            reach_2 = sun_visibility_0(_S40.world_position_6, to_light_7, n_dot_l_4, normal_2, _S42, &(sun_cascade_0), &(sun_fade_0));
        }
        else
        {
            if(_S47 == u32(1))
            {
                var _S49 : u32 = _S46.shadow_tile_0;
                if((_S46.shadow_tile_0) <= u32(8))
                {
                    var _S50 : f32 = point_visibility_0(&(_S46), _S49, _S40.world_position_6, to_light_7, n_dot_l_4, normal_2, _S42);
                    reach_2 = reach_0 * _S50;
                }
                else
                {
                    reach_2 = reach_0;
                }
            }
            else
            {
                var _S51 : u32 = _S46.shadow_tile_0;
                if((_S46.shadow_tile_0) < u32(14))
                {
                    var _S52 : f32 = spot_visibility_0(&(_S46), _S51, _S40.world_position_6, to_light_7, n_dot_l_4, normal_2, _S42);
                    reach_2 = reach_0 * _S52;
                }
                else
                {
                    reach_2 = reach_0;
                }
            }
        }
        direct_0 = direct_0 + _S46.color_0.xyz * vec3<f32>((max(n_dot_l_4, 0.0f) * reach_2));
        slot_1 = slot_1 + u32(1);
    }
    var _S53 : pixelOutput_0 = pixelOutput_0( vec4<f32>(_S40.color_2 * (direct_0 + frame_0.ambient_0.xyz), 1.0f) );
    return _S53;
}

