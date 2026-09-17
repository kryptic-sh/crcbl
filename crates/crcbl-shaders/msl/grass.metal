#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 697 "shaders/grass.slang"
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 651
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 672
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 685
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 827
constant array<float2, int(6)> GRASS_CARD_CORNERS_0 = { float2(0.0f, 0.0f), float2(1.0f, 0.0f), float2(0.0f, 1.0f), float2(1.0f, 0.0f), float2(1.0f, 1.0f), float2(0.0f, 1.0f) };

#line 465
struct GrassBlade_0
{
    float4 root_color_0;
    float4 tip_color_0;
    float4 size_0;
    float4 occlusion_0;
    float4 glow_0;
    float4 patch_0;
    uint4 flags_0;
};


#line 450
struct GrassTile_0
{
    float4 tile_0;
    uint4 slot_0;
};


#line 435
struct GrassParams_0
{
    uint4 limits_0;
    float4 screen_0;
};


#line 435
struct GrassInstance_natural_0
{
    packed_float4 root_0;
    packed_float4 facing_0;
    packed_float4 lean_0;
    packed_float4 ground_0;
    packed_uint4 lanes_0;
};


#line 435
struct GrassBlade_natural_0
{
    packed_float4 root_color_0;
    packed_float4 tip_color_0;
    packed_float4 size_0;
    packed_float4 occlusion_0;
    packed_float4 glow_0;
    packed_float4 patch_0;
    packed_uint4 flags_0;
};


#line 435
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 435
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 435
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(14)> data_2;
};


#line 130
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 view_proj_0;
    float4 camera_position_0;
    float4 ambient_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
    uint4 cluster_grid_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0 light_view_proj_0;
    uint4 probe_counts_0;
    uint4 probe_levels_0;
    array<float4, int(4)> probe_level_origin_0;
    array<float4, int(4)> probe_level_inv_spacing_0;
    array<uint4, int(4)> probe_level_offset_0;
    float4 lod_params_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sky_sh_r_0;
    float4 sky_sh_g_0;
    float4 sky_sh_b_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_view_proj_0;
    uint4 vertex_pool_0;
    array<float4, int(16)> shadow_atlas_rect_0;
    uint4 shadow_filter_0;
};


#line 570
struct GrassField_0
{
    float4 origin_0;
    uint4 tiles_0;
    float4 ground_1;
    uint4 maps_0;
    float4 stack_0;
    array<float4, int(64)> layers_0;
};


#line 1500
struct GpuLight_natural_0
{
    packed_float4 position_0;
    packed_float4 color_0;
    packed_float4 direction_0;
    packed_float4 tangent_0;
    uint kind_0;
    float cos_inner_0;
    uint shadow_tile_0;
    uint flags_1;
};


#line 606
struct WindParams_0
{
    float2 baseDirection_0;
    float baseSpeed_0;
    float gustAmplitude_0;
    float gustPhase_0;
    float invGustWavelength_0;
    float2 pad0_0;
    float2 directionUv_0;
    float2 directionUvPerMetre_0;
    float2 intensityUv_0;
    float2 intensityUvPerMetre_0;
};


#line 606
struct KernelContext_0
{
    GrassTile_0 constant* tile_1;
    GrassParams_0 constant* grass_0;
    GrassInstance_natural_0 device* instances_0;
    GrassBlade_natural_0 device* blades_0;
    FrameUniforms_natural_0 constant* frame_0;
    GrassField_0 constant* field_0;
    texture2d_array<float, access::sample> grassCard_0;
    sampler grassCardSampler_0;
    uint device* cluster_lights_0;
    GpuLight_natural_0 device* lights_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    texture2d<float, access::sample> grassGround_0;
    WindParams_0 constant* wind_0;
    texture2d<float, access::sample> windDirectionLayer_0;
    sampler windSampler_0;
    texture2d<float, access::sample> windIntensityLayer_0;
    GrassInstance_natural_0 device* grassCells_0;
};


#line 1460
GrassBlade_0 grass_row_0(uint index_0, KernelContext_0 thread* kernelContext_0)
{
    GrassBlade_natural_0 _S1 = kernelContext_0->blades_0[min(index_0, max(kernelContext_0->grass_0->limits_0.y, 1U) - 1U)];

#line 1462
    GrassBlade_0 _S2 = { float4(_S1.root_color_0) , float4(_S1.tip_color_0) , float4(_S1.size_0) , float4(_S1.occlusion_0) , float4(_S1.glow_0) , float4(_S1.patch_0) , uint4(_S1.flags_0)  };

#line 1462
    return _S2;
}


#line 769
float grassCardLevel_0(float pixels_0)
{
    float _S3 = 64.0f / max(pixels_0, 0.00009999999747379f);

#line 771
    float level_0 = 0.0f;

#line 771
    uint step_0 = 1U;

    for(;;)
    {

#line 773
        if(step_0 < 7U)
        {
        }
        else
        {

#line 773
            break;
        }
        uint _S4 = step_0 - 1U;

#line 775
        float lower_0 = float(1U << _S4);
        if(_S3 >= lower_0)
        {

#line 776
            level_0 = float(_S4) + min((_S3 - lower_0) / lower_0, 1.0f);

#line 776
        }

#line 773
        step_0 = step_0 + 1U;

#line 773
    }

#line 789
    return min(floor(level_0 + 0.5f), 6.0f);
}


#line 1389
uint grass_hash_0(uint value_0)
{
    uint state_0 = value_0 * 747796405U + 2891336453U;
    uint word_0 = ((state_0 >> ((state_0 >> 28U) + 4U)) ^ state_0) * 277803737U;
    return (word_0 >> 22U) ^ word_0;
}


float2 grass_unit_pair_0(uint lane_0)
{
    return float2(float(lane_0 & 65535U), float((lane_0 >> 16U) & 65535U)) * float2(0.0000152587890625f) ;
}


#line 1446
float grass_patch_0(uint cell_0, KernelContext_0 thread* kernelContext_1)
{
    uint _S5 = max(kernelContext_1->field_0->tiles_0.w, 1U);
    uint _S6 = max(kernelContext_1->field_0->tiles_0.z, 1U);
    uint _S7 = max(kernelContext_1->field_0->tiles_0.x, 1U);
    uint slot_1 = cell_0 / _S5;
    uint lane_1 = cell_0 % _S5;
    uint _S8 = slot_1 % _S7;

#line 1453
    uint _S9 = _S8 * _S6;

#line 1453
    uint _S10 = lane_1 % _S6;

#line 1453
    uint x_0 = _S9 + _S10;
    uint _S11 = slot_1 / _S7;

#line 1454
    uint _S12 = _S11 * _S6;

#line 1454
    uint _S13 = lane_1 / _S6;

    return grass_unit_pair_0(grass_hash_0(((x_0 / 8U * 2376512323U) ^ ((_S12 + _S13) / 8U * 3625334849U)) ^ 374761393U)).x;
}


#line 1472
float3 grass_style_0(float3 color_1, const GrassBlade_natural_0 thread* row_0, float along_0, float patch_1)
{

#line 1472
    float4 _S14 = float4(row_0->occlusion_0) ;

    float reach_0 = _S14.w;

#line 1474
    float occluded_0;
    if(reach_0 > 0.0f)
    {

#line 1475
        occluded_0 = saturate(1.0f - along_0 / reach_0);

#line 1475
    }
    else
    {

#line 1475
        occluded_0 = 0.0f;

#line 1475
    }

#line 1475
    float4 _S15 = float4(row_0->patch_0) ;

#line 1475
    float4 _S16 = float4(row_0->glow_0) ;


    float start_0 = _S16.w;

    return mix(mix(color_1, _S14.xyz, float3(occluded_0) ), _S15.xyz, float3((_S15.w * patch_1)) ) + _S16.xyz * float3(saturate((along_0 - start_0) / max(1.0f - start_0, 0.00009999999747379f))) ;
}


#line 1472
float3 grass_style_1(float3 color_2, const GrassBlade_0 thread* row_1, float along_1, float patch_2)
{

#line 1472
    float4 _S17 = row_1->occlusion_0;

    float reach_1 = row_1->occlusion_0.w;

#line 1474
    float occluded_1;
    if(reach_1 > 0.0f)
    {

#line 1475
        occluded_1 = saturate(1.0f - along_1 / reach_1);

#line 1475
    }
    else
    {

#line 1475
        occluded_1 = 0.0f;

#line 1475
    }


    float start_1 = row_1->glow_0.w;

    return mix(mix(color_2, _S17.xyz, float3(occluded_1) ), row_1->patch_0.xyz, float3((row_1->patch_0.w * patch_2)) ) + row_1->glow_0.xyz * float3(saturate((along_1 - start_1) / max(1.0f - start_1, 0.00009999999747379f))) ;
}


#line 1316
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_2)
{
    uint _S18 = max(kernelContext_2->frame_0->cluster_grid_0.x, 1U);
    uint _S19 = max(kernelContext_2->frame_0->cluster_grid_0.y, 1U);
    uint _S20 = max(kernelContext_2->frame_0->cluster_grid_0.z, 1U);
    uint _S21 = max(kernelContext_2->frame_0->cluster_grid_0.w, 1U);

#line 1326
    uint _S22 = uint(pixel_0.x) / _S21;

#line 1326
    uint _S23 = min(_S22, _S18 - 1U);
    uint _S24 = uint(pixel_0.y) / _S21;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1337
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S20 - 1U))) * _S19 + min(_S24, _S19 - 1U)) * _S18 + _S23;
}


#line 1294
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}

float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}

float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

#line 1313
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 860
float4 atlas_rect_0(uint tile_2, KernelContext_0 thread* kernelContext_3)
{
    return kernelContext_3->frame_0->shadow_atlas_rect_0[tile_2];
}


#line 860
float4 atlas_rect_1(uint tile_3, KernelContext_0 thread* kernelContext_4)
{
    return kernelContext_4->frame_0->shadow_atlas_rect_0[tile_3];
}


#line 875
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 870
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_5)
{
    return rect_1.x / kernelContext_5->frame_0->shadow_params_0.x;
}


#line 837
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 849
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_6)
{

#line 849
    uint _S25;

    if(uint(pixel_1.x) < (kernelContext_6->frame_0->shadow_filter_0.z))
    {

#line 851
        _S25 = kernelContext_6->frame_0->shadow_filter_0.x;

#line 851
    }
    else
    {

#line 851
        _S25 = kernelContext_6->frame_0->shadow_filter_0.y;

#line 851
    }

#line 851
    return _S25;
}


#line 865
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_7)
{
    return kernelContext_7->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 865
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 855
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 880
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_9)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S26 = spoke_0.x;

#line 885
    float _S27 = rotation_0.x;

#line 885
    float _S28 = spoke_0.y;

#line 885
    float _S29 = rotation_0.y;


    float _S30 = ((kernelContext_9->shadow_atlas_0).sample_compare((kernelContext_9->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S26 * _S27 - _S28 * _S29, _S26 * _S29 + _S28 * _S27) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 888
    return _S30;
}


#line 929
float tile_box_pcf_0(uint tile_4, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_10)
{

#line 929
    float4 _S31 = atlas_rect_1(tile_4, kernelContext_10);


    if(atlas_rect_is_empty_0(_S31))
    {
        return 1.0f;
    }

#line 934
    float2 _S32 = atlas_step_1(_S31, kernelContext_10);

#line 934
    int y_0 = int(-1);

#line 934
    float visibility_0 = 0.0f;

#line 939
    for(;;)
    {

#line 939
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 939
            break;
        }

#line 939
        int x_1 = int(-1);

        for(;;)
        {

#line 941
            if(x_1 <= int(1))
            {
            }
            else
            {

#line 941
                break;
            }

#line 941
            float _S33 = tile_tap_0(_S31, _S32, tile_uv_2, float2(float(x_1), float(y_0)), float2(1.0f, 0.0f), reference_1, kernelContext_10);

            float visibility_1 = visibility_0 + _S33;

#line 941
            x_1 = x_1 + int(1);

#line 941
            visibility_0 = visibility_1;

#line 941
        }

#line 939
        y_0 = y_0 + int(1);

#line 939
    }

#line 947
    return visibility_0 / 9.0f;
}


#line 843
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_1 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_1.y * 4U + cell_1.x]];
}


#line 891
float tile_pcf_0(uint tile_5, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_11)
{
    float2 _S34 = shadow_rotation_0(pixel_3);

#line 893
    float4 _S35 = atlas_rect_1(tile_5, kernelContext_11);

    if(atlas_rect_is_empty_0(_S35))
    {
        return 1.0f;
    }

#line 897
    float2 _S36 = atlas_step_1(_S35, kernelContext_11);

#line 897
    uint spot_0 = 0U;

#line 897
    float probe_0 = 0.0f;

#line 902
    for(;;)
    {

#line 902
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 902
            break;
        }

#line 902
        float _S37 = tile_tap_0(_S35, _S36, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S34, reference_2, kernelContext_11);

        float probe_1 = probe_0 + _S37;

#line 902
        spot_0 = spot_0 + 1U;

#line 902
        probe_0 = probe_1;

#line 902
    }

#line 911
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 917
    uint index_1 = 0U;

#line 917
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 921
        if(index_1 < 32U)
        {
        }
        else
        {

#line 921
            break;
        }

#line 921
        float _S38 = tile_tap_0(_S35, _S36, tile_uv_3, SHADOW_DISC_0[index_1] * float2(radius_2) , _S34, reference_2, kernelContext_11);

        float visibility_3 = visibility_2 + _S38;

#line 921
        index_1 = index_1 + 1U;

#line 921
        visibility_2 = visibility_3;

#line 921
    }

#line 926
    return visibility_2 / 32.0f;
}


#line 950
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_12)
{
    float2 texel_0 = kernelContext_12->frame_0->shadow_params_0.xy;

#line 952
    float4 _S39 = atlas_rect_0(cascade_0, kernelContext_12);

#line 952
    float2 _S40 = atlas_step_0(_S39, kernelContext_12);


    float2 _S41 = float2(0.5f, 0.5f) * _S40;


    float2 _S42 = float2(1.0f, 1.0f);

#line 958
    float2 _S43 = _S42 / texel_0;

#line 958
    uint index_2 = 0U;

#line 958
    float sum_0 = 0.0f;

#line 958
    float found_0 = 0.0f;



    for(;;)
    {

#line 962
        if(index_2 < 16U)
        {
        }
        else
        {

#line 962
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_2] * float2(8.0f) ;
        float _S44 = spoke_1.x;

#line 965
        float _S45 = rotation_1.x;

#line 965
        float _S46 = spoke_1.y;

#line 965
        float _S47 = rotation_1.y;

#line 973
        int3 _S48 = int3(int2(min(atlas_uv_0(_S39, clamp(tile_uv_4 + float2(_S44 * _S45 - _S46 * _S47, _S44 * _S47 + _S46 * _S45) * _S40, _S41, float2(1.0f)  - _S41)) * _S43, _S43 - _S42)), int(0));

#line 973
        float depth_1 = ((kernelContext_12->shadow_atlas_0).read(vec<uint,2>(((_S48)).xy), uint(((_S48)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 977
            sum_0 = sum_0 + depth_1;

#line 977
            found_0 = found_1;

#line 974
        }

#line 962
        index_2 = index_2 + 1U;

#line 962
    }

#line 981
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 992
    float _S49 = 2.0f * kernelContext_12->frame_0->cascade_far_0[cascade_0];

#line 992
    float separation_0 = (sum_0 / found_0 - reference_3) * (_S49 + 40.0f);

#line 992
    float _S50 = tile_texels_0(_S39, kernelContext_12);

    return clamp(separation_0 * 0.01999999955296516f / (_S49 / _S50), 2.0f, 8.0f);
}



float cascade_visibility_0(uint cascade_1, float3 world_position_0, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_13)
{

#line 1000
    float4 _S51 = atlas_rect_0(cascade_1, kernelContext_13);

#line 1034
    if(atlas_rect_is_empty_0(_S51))
    {


        return 1.0f;
    }
    float _S52 = 2.0f * kernelContext_13->frame_0->cascade_far_0[cascade_1];

#line 1040
    float _S53 = tile_texels_0(_S51, kernelContext_13);

#line 1040
    float texel_world_0 = _S52 / _S53;

#line 1047
    float4 clip_0 = (((float4(world_position_0 + geometric_normal_1 * float3((texel_world_0 * kernelContext_13->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_13->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1051
    bool _S54;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1052
        _S54 = true;

#line 1052
    }
    else
    {

#line 1052
        _S54 = (ndc_0.z) <= 0.0f;

#line 1052
    }

#line 1052
    if(_S54)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 1062
    uint _S55 = shadow_filter_mode_0(pixel_4, kernelContext_13);

#line 1079
    if(_S55 == 2U)
    {

#line 1079
        float _S56 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_13);

        return _S56;
    }
    if(_S55 == 1U)
    {

#line 1083
        float _S57 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_13);



        return _S57;
    }

    float _S58 = ndc_0.z;

#line 1090
    float _S59 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S58, shadow_rotation_0(pixel_4), kernelContext_13);

#line 1090
    float _S60 = tile_pcf_0(cascade_1, tile_uv_5, _S58, pixel_4, _S59, kernelContext_13);
    return _S60;
}

float sun_visibility_0(float3 world_position_1, float3 to_light_3, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_14)
{
    uint cascade_2;

#line 1096
    bool covered_0;

#line 1105
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 1117
    float eye_distance_0 = length(world_position_1 - kernelContext_14->frame_0->camera_position_0.xyz);

#line 1117
    uint index_3 = 0U;

#line 1125
    for(;;)
    {

#line 1125
        if(index_3 < 2U)
        {
        }
        else
        {

#line 1125
            covered_0 = false;

#line 1125
            cascade_2 = 1U;

#line 1125
            break;
        }
        if(eye_distance_0 < kernelContext_14->frame_0->cascade_far_0[index_3])
        {

#line 1127
            covered_0 = true;

#line 1127
            cascade_2 = index_3;



            break;
        }

#line 1125
        index_3 = index_3 + 1U;

#line 1125
    }

#line 1134
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 1134
    }

#line 1134
    float _S61 = cascade_visibility_0(cascade_2, world_position_1, to_light_3, geometric_normal_2, pixel_5, kernelContext_14);

#line 1141
    uint _S62 = cascade_2 + 1U;

#line 1141
    if(_S62 >= 2U)
    {



        return _S61;
    }

#line 1154
    float band_0 = kernelContext_14->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_14->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S61;
    }

#line 1162
    float _S63 = cascade_visibility_0(_S62, world_position_1, to_light_3, geometric_normal_2, pixel_5, kernelContext_14);

#line 1173
    return mix(_S61, _S63, blend_0);
}


#line 1262
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S64 = axis_1.x;

#line 1265
    float _S65 = axis_1.y;

#line 1265
    bool _S66;

#line 1265
    if(_S64 >= _S65)
    {

#line 1265
        _S66 = _S64 >= (axis_1.z);

#line 1265
    }
    else
    {

#line 1265
        _S66 = false;

#line 1265
    }

#line 1265
    uint _S67;

#line 1265
    if(_S66)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1267
            _S67 = 0U;

#line 1267
        }
        else
        {

#line 1267
            _S67 = 1U;

#line 1267
        }

#line 1267
        return _S67;
    }
    if(_S65 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1271
            _S67 = 2U;

#line 1271
        }
        else
        {

#line 1271
            _S67 = 3U;

#line 1271
        }

#line 1271
        return _S67;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1273
        _S67 = 4U;

#line 1273
    }
    else
    {

#line 1273
        _S67 = 5U;

#line 1273
    }

#line 1273
    return _S67;
}


#line 832
uint light_tile_0(uint tile_6)
{
    return 2U + tile_6;
}


#line 1176
float punctual_visibility_0(uint tile_7, float3 world_position_2, float3 to_light_4, float n_dot_l_1, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_15)
{

    uint atlas_0 = light_tile_0(tile_7);

#line 1179
    float4 _S68 = atlas_rect_0(atlas_0, kernelContext_15);

    if(atlas_rect_is_empty_0(_S68))
    {


        return 1.0f;
    }

#line 1185
    float _S69 = tile_texels_0(_S68, kernelContext_15);

    float texel_world_1 = map_world_0 / _S69;

#line 1197
    float4 clip_1 = (((float4(world_position_2 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(0)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(0)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(0)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(0)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(1)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(1)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(1)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(1)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(2)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(2)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(2)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(2)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(3)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(3)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(3)], (&kernelContext_15->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(3)]))));

#line 1204
    float _S70 = clip_1.w;

#line 1204
    if(_S70 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S70) ;

#line 1208
    bool _S71;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1209
        _S71 = true;

#line 1209
    }
    else
    {

#line 1209
        _S71 = (ndc_1.z) <= 0.0f;

#line 1209
    }

#line 1209
    if(_S71)
    {

#line 1209
        _S71 = true;

#line 1209
    }
    else
    {

#line 1209
        _S71 = (ndc_1.z) > 1.0f;

#line 1209
    }

#line 1209
    if(_S71)
    {

#line 1216
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 1221
    uint _S72 = shadow_filter_mode_0(pixel_6, kernelContext_15);

#line 1230
    if(_S72 == 2U)
    {

#line 1230
        float _S73 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_15);

        return _S73;
    }

#line 1232
    float _S74 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_15);

    return _S74;
}


#line 1276
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_0, float3 world_position_3, float3 to_light_5, float n_dot_l_2, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_16)
{

    if(n_dot_l_2 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_3 - (float4(light_0->position_0) ).xyz;

#line 1284
    float _S75 = punctual_visibility_0(base_0 + point_face_0(from_light_1), world_position_3, to_light_5, n_dot_l_2, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_16);

#line 1290
    return _S75;
}


#line 1237
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_8, float3 world_position_4, float3 to_light_6, float n_dot_l_3, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_17)
{

    if(n_dot_l_3 <= 0.0f)
    {


        return 1.0f;
    }

#line 1244
    float4 _S76 = float4(light_1->direction_0) ;

#line 1251
    float cos_outer_1 = _S76.w;

#line 1251
    float _S77 = punctual_visibility_0(tile_8, world_position_4, to_light_6, n_dot_l_3, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_4 - (float4(light_1->position_0) ).xyz, normalize(_S76.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_17);

#line 1258
    return _S77;
}


#line 1488
float3 grass_light_0(float3 world_position_5, float3 normal_0, float2 pixel_9, KernelContext_0 thread* kernelContext_18)
{

#line 1488
    uint _S78 = froxel_of_0(pixel_9, (((float4(world_position_5, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_18->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_18->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_18->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_18->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_18->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_18->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_18->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_18->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_18->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_18->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_18->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_18->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_18->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_18->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_18->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_18->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w, kernelContext_18);

#line 1499
    uint base_1 = _S78 * 17U;
    uint _S79 = min(kernelContext_18->cluster_lights_0[base_1], 16U);

    float3 _S80 = float3(0.0f, 0.0f, 0.0f);

#line 1502
    uint slot_2 = 0U;

#line 1502
    float3 direct_0 = _S80;
    for(;;)
    {

#line 1503
        if(slot_2 < _S79)
        {
        }
        else
        {

#line 1503
            break;
        }

#line 1503
        thread GpuLight_natural_0 _S81 = kernelContext_18->lights_0[kernelContext_18->cluster_lights_0[base_1 + 1U + slot_2]];

#line 1503
        uint _S82 = (&_S81)->kind_0;


        if(((&_S81)->kind_0) == 3U)
        {



            slot_2 = slot_2 + 1U;

#line 1503
            continue;
        }

#line 1516
        bool _S83 = _S82 == 0U;

#line 1516
        float3 to_light_7;

#line 1516
        float reach_2;

#line 1516
        if(_S83)
        {

#line 1516
            to_light_7 = normalize((float4((&_S81)->direction_0) ).xyz);

#line 1516
            reach_2 = 1.0f;

#line 1516
        }
        else
        {

#line 1516
            float4 _S84 = float4((&_S81)->position_0) ;

#line 1523
            float3 offset_0 = _S84.xyz - world_position_5;
            float distance_2 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
            float reach_3 = punctual_falloff_0(distance_2, _S84.w);
            if(_S82 == 2U)
            {

#line 1527
                float4 _S85 = float4((&_S81)->direction_0) ;

#line 1527
                reach_2 = reach_3 * spot_cone_0(to_light_8, _S85.xyz, _S85.w, (&_S81)->cos_inner_0);

#line 1527
            }
            else
            {

#line 1527
                reach_2 = reach_3;

#line 1527
            }

#line 1527
            to_light_7 = to_light_8;

#line 1516
        }

#line 1534
        float n_dot_l_4 = dot(normal_0, to_light_7);

#line 1534
        float reach_4;
        if(_S83)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 1538
            float _S86 = sun_visibility_0(world_position_5, to_light_7, n_dot_l_4, normal_0, pixel_9, &sun_cascade_0, &sun_fade_0, kernelContext_18);

#line 1538
            reach_4 = _S86;

#line 1535
        }
        else
        {

#line 1542
            if(_S82 == 1U)
            {

#line 1542
                uint _S87 = (&_S81)->shadow_tile_0;

                if(((&_S81)->shadow_tile_0) <= 8U)
                {

#line 1544
                    float _S88 = point_visibility_0(&_S81, _S87, world_position_5, to_light_7, n_dot_l_4, normal_0, pixel_9, kernelContext_18);

#line 1544
                    reach_4 = reach_2 * _S88;

#line 1544
                }
                else
                {

#line 1544
                    reach_4 = reach_2;

#line 1544
                }

#line 1542
            }
            else
            {

#line 1542
                uint _S89 = (&_S81)->shadow_tile_0;

#line 1550
                if(((&_S81)->shadow_tile_0) < 14U)
                {

#line 1550
                    float _S90 = spot_visibility_0(&_S81, _S89, world_position_5, to_light_7, n_dot_l_4, normal_0, pixel_9, kernelContext_18);

#line 1550
                    reach_4 = reach_2 * _S90;

#line 1550
                }
                else
                {

#line 1550
                    reach_4 = reach_2;

#line 1550
                }

#line 1542
            }

#line 1535
        }

#line 1535
        direct_0 = direct_0 + (float4((&_S81)->color_0) ).xyz * float3((max(n_dot_l_4, 0.0f) * reach_4)) ;

#line 1503
        slot_2 = slot_2 + 1U;

#line 1503
    }

#line 1559
    return direct_0 + kernelContext_18->frame_0->ambient_0.xyz;
}


#line 1559
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1559
struct pixelInput_0
{
    float3 world_position_6 [[user(WORLD_POSITION)]];
    float3 normal_1 [[user(NORMAL)]];
    float3 color_3 [[user(COLOR)]];
    float2 uv_0 [[user(TEXCOORD)]];
    float level_1 [[user(CARD_LEVEL)]];
    [[flat]] uint row_2 [[user(BLADE_ROW)]];
    [[flat]] float patch_3 [[user(PATCH)]];
};


#line 1636
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S91 [[stage_in]], float4 position_1 [[position]], GrassTile_0 constant* tile_9 [[buffer(2)]], GrassParams_0 constant* grass_1 [[buffer(1)]], GrassInstance_natural_0 device* instances_1 [[buffer(3)]], GrassBlade_natural_0 device* blades_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GrassField_0 constant* field_1 [[buffer(7)]], texture2d_array<float, access::sample> grassCard_1 [[texture(0)]], sampler grassCardSampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(6)]], GpuLight_natural_0 device* lights_1 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> grassGround_1 [[texture(2)]], WindParams_0 constant* wind_1 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_1 [[texture(3)]], sampler windSampler_1 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_1 [[texture(4)]], GrassInstance_natural_0 device* grassCells_1 [[buffer(8)]])
{

#line 1636
    thread KernelContext_0 kernelContext_19;

#line 1636
    (&kernelContext_19)->tile_1 = tile_9;

#line 1636
    (&kernelContext_19)->grass_0 = grass_1;

#line 1636
    (&kernelContext_19)->instances_0 = instances_1;

#line 1636
    (&kernelContext_19)->blades_0 = blades_1;

#line 1636
    (&kernelContext_19)->frame_0 = frame_1;

#line 1636
    (&kernelContext_19)->field_0 = field_1;

#line 1636
    (&kernelContext_19)->grassCard_0 = grassCard_1;

#line 1636
    (&kernelContext_19)->grassCardSampler_0 = grassCardSampler_1;

#line 1636
    (&kernelContext_19)->cluster_lights_0 = cluster_lights_1;

#line 1636
    (&kernelContext_19)->lights_0 = lights_1;

#line 1636
    (&kernelContext_19)->shadow_atlas_0 = shadow_atlas_1;

#line 1636
    (&kernelContext_19)->shadow_sampler_0 = shadow_sampler_1;

#line 1636
    (&kernelContext_19)->grassGround_0 = grassGround_1;

#line 1636
    (&kernelContext_19)->wind_0 = wind_1;

#line 1636
    (&kernelContext_19)->windDirectionLayer_0 = windDirectionLayer_1;

#line 1636
    (&kernelContext_19)->windSampler_0 = windSampler_1;

#line 1636
    (&kernelContext_19)->windIntensityLayer_0 = windIntensityLayer_1;

#line 1636
    (&kernelContext_19)->grassCells_0 = grassCells_1;

#line 1642
    float _S92 = _S91.uv_0.y;

#line 1642
    float3 _S93 = float3(_S91.uv_0.x, 1.0f - _S92, 0.0f);

    if((((grassCard_1).sample((grassCardSampler_1), ((_S93)).xy, uint(((_S93)).z), level((_S91.level_1)))).x) < 0.5f)
    {
        discard_fragment();

#line 1644
    }

#line 1644
    thread GrassBlade_natural_0 _S94 = (&kernelContext_19)->blades_0[_S91.row_2];

#line 1644
    float3 _S95 = grass_style_0(_S91.color_3, &_S94, _S92, _S91.patch_3);

#line 1644
    float3 _S96 = grass_light_0(_S91.world_position_6, normalize(_S91.normal_1), position_1.xy, &kernelContext_19);

#line 1644
    pixelOutput_0 _S97 = { float4(_S95 * _S96, 1.0f) };

#line 1652
    return _S97;
}


#line 1707
uint grass_shell_count_0(KernelContext_0 thread* kernelContext_20)
{
    return clamp(uint(kernelContext_20->field_0->stack_0.y + 0.5f), 1U, 64U);
}


#line 1412
float grass_ground_texel_0(int2 texel_1, KernelContext_0 thread* kernelContext_21)
{

    int3 _S98 = int3(clamp(texel_1, int2(int(0), int(0)), int2(int(kernelContext_21->field_0->maps_0.x) - int(1), int(kernelContext_21->field_0->maps_0.y) - int(1))), int(0));

#line 1415
    return ((kernelContext_21->grassGround_0).read(vec<uint,2>(((_S98)).xy), uint(((_S98)).z)).x);
}


#line 1403
struct GrassGround_0
{
    float height_0;
    float3 normal_2;
};


#line 1420
GrassGround_0 grass_ground_under_0(float2 world_0, KernelContext_0 thread* kernelContext_22)
{
    float2 at_0 = (world_0 - kernelContext_22->field_0->ground_1.xy) * float2(kernelContext_22->field_0->ground_1.w) ;
    float2 base_2 = floor(at_0);
    float2 blend_1 = at_0 - base_2;
    int2 _S99 = int2(base_2);

#line 1425
    float _S100 = grass_ground_texel_0(_S99, kernelContext_22);

#line 1425
    float _S101 = grass_ground_texel_0(_S99 + int2(int(1), int(0)), kernelContext_22);

#line 1425
    float _S102 = grass_ground_texel_0(_S99 + int2(int(0), int(1)), kernelContext_22);

#line 1425
    float _S103 = grass_ground_texel_0(_S99 + int2(int(1), int(1)), kernelContext_22);

#line 1432
    float _S104 = blend_1.x;

#line 1432
    float _S105 = _S101 - _S100;

#line 1432
    float lower_1 = _S100 + _S104 * _S105;
    float _S106 = _S103 - _S102;

#line 1431
    thread GrassGround_0 under_0;


    (&under_0)->height_0 = lower_1 + blend_1.y * (_S102 + _S104 * _S106 - lower_1);


    (&under_0)->normal_2 = normalize(float3(- (0.5f * (_S105 + _S106)), kernelContext_22->field_0->ground_1.z, - (0.5f * (_S102 - _S100 + (_S103 - _S101)))));
    return under_0;
}


#line 1342
float windSmoothTriangle_0(float u_0)
{
    float s_0 = abs(fract(u_0 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}


float3 windSample_0(float3 posRel_0, KernelContext_0 thread* kernelContext_23)
{


    float2 _S107 = posRel_0.xz;


    float2 deflection_0 = ((kernelContext_23->windDirectionLayer_0).sample((kernelContext_23->windSampler_0), (kernelContext_23->wind_0->directionUv_0 + _S107 * kernelContext_23->wind_0->directionUvPerMetre_0), level((0.0f)))).xy * float2(2.0f)  - float2(1.0f) ;



    float intensity_0 = ((kernelContext_23->windIntensityLayer_0).sample((kernelContext_23->windSampler_0), (kernelContext_23->wind_0->intensityUv_0 + _S107 * kernelContext_23->wind_0->intensityUvPerMetre_0), level((0.0f)))).x;

    float2 base_3 = kernelContext_23->wind_0->baseDirection_0;
    float _S108 = kernelContext_23->wind_0->baseDirection_0.x;

#line 1363
    float _S109 = deflection_0.x;

#line 1363
    float _S110 = kernelContext_23->wind_0->baseDirection_0.y;

#line 1363
    float _S111 = deflection_0.y;

#line 1363
    float2 turned_0 = float2(_S108 * _S109 - _S110 * _S111, _S108 * _S111 + _S110 * _S109);

    float lengthSquared_0 = dot(turned_0, turned_0);

#line 1365
    float2 direction_1;

    if(lengthSquared_0 > 9.999999960041972e-13f)
    {

#line 1367
        direction_1 = turned_0 / float2(sqrt(lengthSquared_0)) ;

#line 1367
    }
    else
    {

#line 1367
        direction_1 = base_3;

#line 1367
    }

#line 1372
    float speed_0 = intensity_0 * kernelContext_23->wind_0->baseSpeed_0 * (1.0f + kernelContext_23->wind_0->gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S107, base_3) * kernelContext_23->wind_0->invGustWavelength_0 + kernelContext_23->wind_0->gustPhase_0) - 1.0f));
    return float3(direction_1.x * speed_0, 0.0f, direction_1.y * speed_0);
}



float3 grass_lean_0(float3 velocity_0, float height_1)
{
    float speed_1 = length(velocity_0);
    float bend_0 = height_1 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    float2 along_2 = velocity_0.xz * float2((bend_0 / max(speed_1, 9.99999997475242708e-07f))) ;
    float _S112 = bend_0 * bend_0;
    return float3(along_2.x, - (_S112 / (height_1 + sqrt(max(height_1 * height_1 - _S112, 0.0f)))), along_2.y);
}


#line 1656
struct GrassShellVertex_0
{
    float4 position_2;
    float3 world_position_7;
    float3 rest_0;
    float3 normal_3;
    float footprint_0;
    float occlusion_1;
    [[flat]] float4 fin_0;
};


#line 1680
GrassShellVertex_0 grass_sheet_vertex_0(float2 rest_1, float share_0, float4 fin_1, float occlusion_2, KernelContext_0 thread* kernelContext_24)
{

#line 1680
    GrassGround_0 _S113 = grass_ground_under_0(rest_1, kernelContext_24);


    float lift_0 = share_0 * kernelContext_24->field_0->stack_0.x;
    float _S114 = rest_1.x;

#line 1684
    float _S115 = rest_1.y;

#line 1684
    float3 ground_2 = float3(_S114, _S113.height_0, _S115);

#line 1684
    float3 _S116 = windSample_0(ground_2 - kernelContext_24->frame_0->camera_position_0.xyz, kernelContext_24);

#line 1691
    float3 world_1 = ground_2 + float3(0.0f, lift_0, 0.0f) + grass_lean_0(_S116, kernelContext_24->field_0->stack_0.x) * float3((share_0 * share_0)) ;

    thread GrassShellVertex_0 output_1;
    float4 _S117 = (((float4(world_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_24->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_24->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_24->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_24->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_24->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_24->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_24->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_24->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_24->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_24->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_24->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_24->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_24->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_24->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_24->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_24->frame_0->view_proj_0.data_0[int(3)][int(3)]))));

#line 1694
    (&output_1)->position_2 = _S117;
    (&output_1)->world_position_7 = world_1;
    (&output_1)->rest_0 = float3(_S114, lift_0, _S115);
    (&output_1)->normal_3 = _S113.normal_2;


    (&output_1)->footprint_0 = max(abs(_S117.w), 0.00009999999747379f) / max(kernelContext_24->grass_0->screen_0.x, 0.00009999999747379f);
    (&output_1)->occlusion_1 = occlusion_2;
    (&output_1)->fin_0 = fin_1;
    return output_1;
}


#line 1729
float grass_occlusion_at_0(float share_1, KernelContext_0 thread* kernelContext_25)
{

#line 1729
    uint _S118 = grass_shell_count_0(kernelContext_25);

#line 1729
    float occlusion_3 = kernelContext_25->field_0->layers_0[int(0)].y;

#line 1729
    uint shell_0 = 0U;



    for(;;)
    {

#line 1733
        if(shell_0 < _S118)
        {
        }
        else
        {

#line 1733
            break;
        }
        if((kernelContext_25->field_0->layers_0[shell_0].x) <= share_1)
        {

#line 1735
            occlusion_3 = kernelContext_25->field_0->layers_0[shell_0].y;

#line 1735
        }

#line 1733
        shell_0 = shell_0 + 1U;

#line 1733
    }

#line 1740
    return occlusion_3;
}


#line 493
struct GrassInstance_0
{
    float4 root_0;
    float4 facing_0;
    float4 lean_0;
    float4 ground_0;
    uint4 lanes_0;
};


#line 1807
GrassInstance_0 grass_no_blade_0()
{
    thread GrassInstance_0 none_0;
    float4 _S119 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 1810
    (&none_0)->root_0 = _S119;
    (&none_0)->facing_0 = _S119;
    (&none_0)->lean_0 = _S119;
    (&none_0)->ground_0 = _S119;
    (&none_0)->lanes_0 = uint4(0U, 0U, 0U, 0U);
    return none_0;
}



GrassInstance_0 grass_cell_0(int2 global_0, KernelContext_0 thread* kernelContext_26)
{
    int side_0 = int(max(kernelContext_26->field_0->tiles_0.z, 1U));
    int2 extent_0 = int2(int(kernelContext_26->field_0->tiles_0.x), int(kernelContext_26->field_0->tiles_0.y)) * int2(side_0) ;
    int _S120 = global_0.x;

#line 1824
    bool _S121;

#line 1824
    if(_S120 < int(0))
    {

#line 1824
        _S121 = true;

#line 1824
    }
    else
    {

#line 1824
        _S121 = (global_0.y) < int(0);

#line 1824
    }

#line 1824
    if(_S121)
    {

#line 1824
        _S121 = true;

#line 1824
    }
    else
    {

#line 1824
        _S121 = _S120 >= (extent_0.x);

#line 1824
    }

#line 1824
    if(_S121)
    {

#line 1824
        _S121 = true;

#line 1824
    }
    else
    {

#line 1824
        _S121 = (global_0.y) >= (extent_0.y);

#line 1824
    }

#line 1824
    if(_S121)
    {
        return grass_no_blade_0();
    }
    uint2 _S122 = uint2(global_0);
    uint cells_0 = uint(side_0);
    uint _S123 = _S122.y;

#line 1830
    uint _S124 = _S123 / cells_0;

#line 1830
    uint _S125 = _S124 * kernelContext_26->field_0->tiles_0.x;

#line 1830
    uint _S126 = _S122.x;

#line 1830
    uint _S127 = _S126 / cells_0;

#line 1830
    uint slot_3 = _S125 + _S127;
    uint _S128 = _S123 % cells_0;

#line 1831
    uint _S129 = _S128 * cells_0;

#line 1831
    uint _S130 = _S126 % cells_0;
    GrassInstance_natural_0 _S131 = kernelContext_26->grassCells_0[slot_3 * kernelContext_26->field_0->tiles_0.w + (_S129 + _S130)];

#line 1832
    GrassInstance_0 _S132 = { float4(_S131.root_0) , float4(_S131.facing_0) , float4(_S131.lean_0) , float4(_S131.ground_0) , uint4(_S131.lanes_0)  };

#line 1832
    return _S132;
}




float grass_strand_reach_0(const GrassInstance_0 thread* blade_0, float lift_1, float floor_reach_0, float fade_1, KernelContext_0 thread* kernelContext_27)
{
    float _S133 = blade_0->root_0.w;

#line 1840
    bool _S134;

#line 1840
    if(!(lift_1 < _S133))
    {

#line 1840
        _S134 = true;

#line 1840
    }
    else
    {

#line 1840
        GrassBlade_0 _S135 = grass_row_0(blade_0->lanes_0.y, kernelContext_27);

#line 1840
        _S134 = (_S135.flags_0.x) != 1U;

#line 1840
    }

#line 1840
    if(_S134)
    {
        return -1.0f;
    }
    float bound_0 = 0.5f * kernelContext_27->field_0->origin_0.w;

    return min(max(min(blade_0->facing_0.z, bound_0) * (1.0f - lift_1 / _S133), floor_reach_0), bound_0) * fade_1;
}


#line 1846
struct pixelOutput_1
{
    float4 output_2 [[color(0)]];
};


#line 1846
struct pixelInput_1
{
    float3 world_position_8 [[user(WORLD_POSITION)]];
    float3 rest_2 [[user(REST_POSITION)]];
    float3 normal_4 [[user(NORMAL)]];
    float footprint_1 [[user(FOOTPRINT)]];
    float occlusion_4 [[user(OCCLUSION)]];
    [[flat]] float4 fin_2 [[user(FIN)]];
};


#line 1851
[[fragment]] pixelOutput_1 shellFragmentMain(pixelInput_1 _S136 [[stage_in]], float4 position_3 [[position]], GrassTile_0 constant* tile_10 [[buffer(2)]], GrassParams_0 constant* grass_2 [[buffer(1)]], GrassInstance_natural_0 device* instances_2 [[buffer(3)]], GrassBlade_natural_0 device* blades_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GrassField_0 constant* field_2 [[buffer(7)]], texture2d_array<float, access::sample> grassCard_2 [[texture(0)]], sampler grassCardSampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(6)]], GpuLight_natural_0 device* lights_2 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> grassGround_2 [[texture(2)]], WindParams_0 constant* wind_2 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_2 [[texture(3)]], sampler windSampler_2 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_2 [[texture(4)]], GrassInstance_natural_0 device* grassCells_2 [[buffer(8)]])
{

#line 1851
    thread KernelContext_0 kernelContext_28;

#line 1851
    (&kernelContext_28)->tile_1 = tile_10;

#line 1851
    (&kernelContext_28)->grass_0 = grass_2;

#line 1851
    (&kernelContext_28)->instances_0 = instances_2;

#line 1851
    (&kernelContext_28)->blades_0 = blades_2;

#line 1851
    (&kernelContext_28)->frame_0 = frame_2;

#line 1851
    (&kernelContext_28)->field_0 = field_2;

#line 1851
    (&kernelContext_28)->grassCard_0 = grassCard_2;

#line 1851
    (&kernelContext_28)->grassCardSampler_0 = grassCardSampler_2;

#line 1851
    (&kernelContext_28)->cluster_lights_0 = cluster_lights_2;

#line 1851
    (&kernelContext_28)->lights_0 = lights_2;

#line 1851
    (&kernelContext_28)->shadow_atlas_0 = shadow_atlas_2;

#line 1851
    (&kernelContext_28)->shadow_sampler_0 = shadow_sampler_2;

#line 1851
    (&kernelContext_28)->grassGround_0 = grassGround_2;

#line 1851
    (&kernelContext_28)->wind_0 = wind_2;

#line 1851
    (&kernelContext_28)->windDirectionLayer_0 = windDirectionLayer_2;

#line 1851
    (&kernelContext_28)->windSampler_0 = windSampler_2;

#line 1851
    (&kernelContext_28)->windIntensityLayer_0 = windIntensityLayer_2;

#line 1851
    (&kernelContext_28)->grassCells_0 = grassCells_2;

    float cell_2 = field_2->origin_0.w;
    float2 _S137 = _S136.rest_2.xz;

#line 1854
    float2 from_corner_0 = (_S137 - field_2->origin_0.xy) / float2(cell_2) ;
    float2 _S138 = floor(from_corner_0);

#line 1855
    int2 _S139 = int2(_S138);
    float2 inside_0 = from_corner_0 - _S138;

#line 1856
    int _S140;
    if((inside_0.x) < 0.5f)
    {

#line 1857
        _S140 = int(-1);

#line 1857
    }
    else
    {

#line 1857
        _S140 = int(1);

#line 1857
    }

#line 1857
    int band_1;

#line 1857
    if((inside_0.y) < 0.5f)
    {

#line 1857
        band_1 = int(-1);

#line 1857
    }
    else
    {

#line 1857
        band_1 = int(1);

#line 1857
    }
    float lift_2 = _S136.rest_2.y;


    float _S141 = 0.5f * _S136.footprint_1;
    float _S142 = _S136.fin_2.y;

    GrassInstance_0 _S143 = grass_no_blade_0();

    float _S144 = _S136.fin_2.x;

#line 1866
    int dz_0;

#line 1866
    int dx_0;

#line 1866
    float nearest_0;

#line 1866
    float nearest_1;

#line 1866
    GrassInstance_0 found_2;

#line 1866
    GrassInstance_0 found_3;

#line 1866
    bool _S145;

#line 1866
    if(_S144 < 0.5f)
    {

#line 1866
        nearest_1 = 2.0f;

#line 1866
        found_3 = _S143;

#line 1866
        dz_0 = int(0);


        for(;;)
        {

#line 1869
            if(dz_0 < int(2))
            {
            }
            else
            {

#line 1869
                break;
            }

#line 1869
            nearest_0 = nearest_1;

#line 1869
            found_2 = found_3;

#line 1869
            dx_0 = int(0);

            for(;;)
            {

#line 1871
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 1871
                    break;
                }

#line 1871
                GrassInstance_0 _S146 = grass_cell_0(_S139 + int2(dx_0 * _S140, dz_0 * band_1), &kernelContext_28);

#line 1871
                thread GrassInstance_0 _S147 = _S146;

#line 1871
                float _S148 = grass_strand_reach_0(&_S147, lift_2, _S141, _S142, &kernelContext_28);



                float2 offset_1 = _S137 - _S146.root_0.xz;
                float apart_0 = sqrt(dot(offset_1, offset_1)) / max(_S148, 9.99999997475242708e-07f);
                if(_S148 > 0.0f)
                {

#line 1877
                    _S145 = apart_0 < nearest_0;

#line 1877
                }
                else
                {

#line 1877
                    _S145 = false;

#line 1877
                }

#line 1877
                if(_S145)
                {

#line 1877
                    nearest_0 = apart_0;

#line 1877
                    found_2 = _S146;

#line 1877
                }

#line 1871
                dx_0 = dx_0 + int(1);

#line 1871
            }

#line 1869
            int dz_1 = dz_0 + int(1);

#line 1869
            nearest_1 = nearest_0;

#line 1869
            found_3 = found_2;

#line 1869
            dz_0 = dz_1;

#line 1869
        }

#line 1869
        nearest_0 = nearest_1;

#line 1869
        found_2 = found_3;

#line 1866
    }
    else
    {

#line 1889
        bool along_z_0 = _S144 < 1.5f;
        float _S149 = _S136.fin_2.z;

#line 1890
        if(along_z_0)
        {

#line 1890
            nearest_0 = field_2->origin_0.x;

#line 1890
        }
        else
        {

#line 1890
            nearest_0 = field_2->origin_0.y;

#line 1890
        }
        int _S150 = int(floor((_S149 - nearest_0) / cell_2 + 0.5f)) - int(2);
        if(along_z_0)
        {

#line 1892
            dz_0 = _S139.y;

#line 1892
        }
        else
        {

#line 1892
            dz_0 = _S139.x;

#line 1892
        }
        if(along_z_0)
        {

#line 1893
            _S140 = band_1;

#line 1893
        }

#line 1893
        nearest_0 = 2.0f;

#line 1893
        found_2 = _S143;

#line 1893
        band_1 = int(0);
        for(;;)
        {

#line 1894
            if(band_1 < int(4))
            {
            }
            else
            {

#line 1894
                break;
            }

#line 1894
            nearest_1 = nearest_0;

#line 1894
            found_3 = found_2;

#line 1894
            dx_0 = int(0);

            for(;;)
            {

#line 1896
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 1896
                    break;
                }
                int along_3 = dz_0 + dx_0 * _S140;

#line 1898
                int2 _S151;
                if(along_z_0)
                {

#line 1899
                    _S151 = int2(_S150 + band_1, along_3);

#line 1899
                }
                else
                {

#line 1899
                    _S151 = int2(along_3, _S150 + band_1);

#line 1899
                }

#line 1899
                GrassInstance_0 _S152 = grass_cell_0(_S151, &kernelContext_28);

#line 1899
                thread GrassInstance_0 _S153 = _S152;

#line 1899
                float _S154 = grass_strand_reach_0(&_S153, lift_2, _S141, _S142, &kernelContext_28);

#line 1899
                float offset_2;



                if(along_z_0)
                {

#line 1903
                    offset_2 = _S136.rest_2.z - _S152.root_0.z;

#line 1903
                }
                else
                {

#line 1903
                    offset_2 = _S136.rest_2.x - _S152.root_0.x;

#line 1903
                }
                float apart_1 = abs(offset_2) / max(_S154, 9.99999997475242708e-07f);
                if(_S154 > 0.0f)
                {

#line 1905
                    _S145 = apart_1 < nearest_1;

#line 1905
                }
                else
                {

#line 1905
                    _S145 = false;

#line 1905
                }

#line 1905
                if(_S145)
                {

#line 1905
                    nearest_1 = apart_1;

#line 1905
                    found_3 = _S152;

#line 1905
                }

#line 1896
                dx_0 = dx_0 + int(1);

#line 1896
            }

#line 1894
            int band_2 = band_1 + int(1);

#line 1894
            nearest_0 = nearest_1;

#line 1894
            found_2 = found_3;

#line 1894
            band_1 = band_2;

#line 1894
        }

#line 1866
    }

#line 1913
    if(!(nearest_0 < 1.0f))
    {
        discard_fragment();

#line 1913
    }

#line 1913
    GrassBlade_0 _S155 = grass_row_0(found_2.lanes_0.y, &kernelContext_28);

#line 1919
    float along_blade_0 = saturate(lift_2 / found_2.root_0.w);

    float3 color_4 = mix(_S155.root_color_0.xyz, _S155.tip_color_0.xyz, float3(along_blade_0) ) * float3((1.0f - 0.18000000715255737f * found_2.facing_0.w)) ;

#line 1921
    float _S156 = grass_patch_0(found_2.lanes_0.x, &kernelContext_28);

#line 1921
    thread GrassBlade_0 _S157 = _S155;

#line 1921
    float3 _S158 = grass_style_1(color_4, &_S157, along_blade_0, _S156);
    float3 color_5 = _S158 * float3(_S136.occlusion_4) ;

#line 1922
    float3 normal_5;

    if((_S155.flags_0.y) == 1U)
    {

#line 1924
        normal_5 = float3(0.0f, 1.0f, 0.0f);

#line 1924
    }
    else
    {

#line 1924
        normal_5 = normalize(_S136.normal_4);

#line 1924
    }

#line 1924
    float3 _S159 = grass_light_0(_S136.world_position_8, normal_5, position_3.xy, &kernelContext_28);

#line 1924
    pixelOutput_1 _S160 = { float4(color_5 * _S159, 1.0f) };
    return _S160;
}


#line 1925
struct vertexMain_Result_0
{
    float4 position_4 [[position]];
    float3 world_position_9 [[user(WORLD_POSITION)]];
    float3 normal_6 [[user(NORMAL)]];
    float3 color_6 [[user(COLOR)]];
    float2 uv_1 [[user(TEXCOORD)]];
    float level_2 [[user(CARD_LEVEL)]];
    uint row_3 [[user(BLADE_ROW)]];
    float patch_4 [[user(PATCH)]];
};


#line 1563
struct GrassVertex_0
{
    float4 position_5;
    float3 world_position_10;
    float3 normal_7;
    float3 color_7;
    float2 uv_2;
    float level_3;
    [[flat]] uint row_4;
    [[flat]] float patch_5;
};


#line 1563
[[vertex]] vertexMain_Result_0 vertexMain(uint index_4 [[vertex_id]], uint instance_id_0 [[instance_id]], GrassTile_0 constant* tile_11 [[buffer(2)]], GrassParams_0 constant* grass_3 [[buffer(1)]], GrassInstance_natural_0 device* instances_3 [[buffer(3)]], GrassBlade_natural_0 device* blades_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], GrassField_0 constant* field_3 [[buffer(7)]], texture2d_array<float, access::sample> grassCard_3 [[texture(0)]], sampler grassCardSampler_3 [[sampler(0)]], uint device* cluster_lights_3 [[buffer(6)]], GpuLight_natural_0 device* lights_3 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> grassGround_3 [[texture(2)]], WindParams_0 constant* wind_3 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_3 [[texture(3)]], sampler windSampler_3 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_3 [[texture(4)]], GrassInstance_natural_0 device* grassCells_3 [[buffer(8)]])
{

#line 1563
    thread KernelContext_0 kernelContext_29;

#line 1563
    (&kernelContext_29)->tile_1 = tile_11;

#line 1563
    (&kernelContext_29)->grass_0 = grass_3;

#line 1563
    (&kernelContext_29)->instances_0 = instances_3;

#line 1563
    (&kernelContext_29)->blades_0 = blades_3;

#line 1563
    (&kernelContext_29)->frame_0 = frame_3;

#line 1563
    (&kernelContext_29)->field_0 = field_3;

#line 1563
    (&kernelContext_29)->grassCard_0 = grassCard_3;

#line 1563
    (&kernelContext_29)->grassCardSampler_0 = grassCardSampler_3;

#line 1563
    (&kernelContext_29)->cluster_lights_0 = cluster_lights_3;

#line 1563
    (&kernelContext_29)->lights_0 = lights_3;

#line 1563
    (&kernelContext_29)->shadow_atlas_0 = shadow_atlas_3;

#line 1563
    (&kernelContext_29)->shadow_sampler_0 = shadow_sampler_3;

#line 1563
    (&kernelContext_29)->grassGround_0 = grassGround_3;

#line 1563
    (&kernelContext_29)->wind_0 = wind_3;

#line 1563
    (&kernelContext_29)->windDirectionLayer_0 = windDirectionLayer_3;

#line 1563
    (&kernelContext_29)->windSampler_0 = windSampler_3;

#line 1563
    (&kernelContext_29)->windIntensityLayer_0 = windIntensityLayer_3;

#line 1563
    (&kernelContext_29)->grassCells_0 = grassCells_3;

#line 1593
    GrassInstance_natural_0 blade_1 = instances_3[tile_11->slot_0.x * grass_3->limits_0.x + instance_id_0];

#line 1593
    uint4 _S161 = uint4(blade_1.lanes_0) ;
    uint _S162 = _S161.y;

#line 1594
    GrassBlade_0 _S163 = grass_row_0(_S162, &kernelContext_29);


    uint _S164 = index_4 % 6U;

#line 1597
    float4 _S165 = float4(blade_1.facing_0) ;

#line 1602
    float2 facing_1 = _S165.xy;

#line 1602
    float2 across_0;
    if((index_4 / 6U) == 0U)
    {

#line 1603
        across_0 = facing_1;

#line 1603
    }
    else
    {

#line 1603
        across_0 = float2(- facing_1.y, facing_1.x);

#line 1603
    }

    float _S166 = _S165.z;

#line 1610
    float _S167 = GRASS_CARD_CORNERS_0[_S164].y;

#line 1610
    float4 _S168 = float4(blade_1.root_0) ;
    float3 world_2 = _S168.xyz + float3(across_0.x, 0.0f, across_0.y) * float3((_S166 * (GRASS_CARD_CORNERS_0[_S164].x * 2.0f - 1.0f)))  + float3(0.0f, _S168.w * _S167, 0.0f) + (float4(blade_1.lean_0) ).xyz * float3((_S167 * _S167)) ;

    thread GrassVertex_0 output_3;
    (&output_3)->position_5 = (((float4(world_2, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_29)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_29)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_3)->world_position_10 = world_2;

#line 1615
    float3 _S169;
    if((_S163.flags_0.y) == 1U)
    {

#line 1616
        _S169 = float3(0.0f, 1.0f, 0.0f);

#line 1616
    }
    else
    {

#line 1616
        _S169 = (float4(blade_1.ground_0) ).xyz;

#line 1616
    }

#line 1616
    (&output_3)->normal_7 = _S169;

#line 1621
    (&output_3)->color_7 = mix(_S163.root_color_0.xyz, _S163.tip_color_0.xyz, float3(_S167) ) * float3((1.0f - 0.18000000715255737f * _S165.w)) ;
    (&output_3)->uv_2 = GRASS_CARD_CORNERS_0[_S164];

#line 1627
    (&output_3)->level_3 = grassCardLevel_0(2.0f * _S166 * (&kernelContext_29)->grass_0->screen_0.x / max(abs((&output_3)->position_5.w), 0.00009999999747379f));

    (&output_3)->row_4 = min(_S162, max(grass_3->limits_0.y, 1U) - 1U);

#line 1629
    float _S170 = grass_patch_0(_S161.x, &kernelContext_29);
    (&output_3)->patch_5 = _S170;
    GrassVertex_0 _S171 = output_3;

#line 1631
    thread vertexMain_Result_0 _S172;

#line 1631
    (&_S172)->position_4 = _S171.position_5;

#line 1631
    (&_S172)->world_position_9 = _S171.world_position_10;

#line 1631
    (&_S172)->normal_6 = _S171.normal_7;

#line 1631
    (&_S172)->color_6 = _S171.color_7;

#line 1631
    (&_S172)->uv_1 = _S171.uv_2;

#line 1631
    (&_S172)->level_2 = _S171.level_3;

#line 1631
    (&_S172)->row_3 = _S171.row_4;

#line 1631
    (&_S172)->patch_4 = _S171.patch_5;

#line 1631
    return _S172;
}


#line 1631
struct shellVertexMain_Result_0
{
    float4 position_6 [[position]];
    float3 world_position_11 [[user(WORLD_POSITION)]];
    float3 rest_3 [[user(REST_POSITION)]];
    float3 normal_8 [[user(NORMAL)]];
    float footprint_2 [[user(FOOTPRINT)]];
    float occlusion_5 [[user(OCCLUSION)]];
    float4 fin_3 [[user(FIN)]];
};


#line 1631
[[vertex]] shellVertexMain_Result_0 shellVertexMain(uint index_5 [[vertex_id]], uint instance_id_1 [[instance_id]], GrassTile_0 constant* tile_12 [[buffer(2)]], GrassParams_0 constant* grass_4 [[buffer(1)]], GrassInstance_natural_0 device* instances_4 [[buffer(3)]], GrassBlade_natural_0 device* blades_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_4 [[buffer(0)]], GrassField_0 constant* field_4 [[buffer(7)]], texture2d_array<float, access::sample> grassCard_4 [[texture(0)]], sampler grassCardSampler_4 [[sampler(0)]], uint device* cluster_lights_4 [[buffer(6)]], GpuLight_natural_0 device* lights_4 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> grassGround_4 [[texture(2)]], WindParams_0 constant* wind_4 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_4 [[texture(3)]], sampler windSampler_4 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_4 [[texture(4)]], GrassInstance_natural_0 device* grassCells_4 [[buffer(8)]])
{

#line 1631
    thread KernelContext_0 kernelContext_30;

#line 1631
    (&kernelContext_30)->tile_1 = tile_12;

#line 1631
    (&kernelContext_30)->grass_0 = grass_4;

#line 1631
    (&kernelContext_30)->instances_0 = instances_4;

#line 1631
    (&kernelContext_30)->blades_0 = blades_4;

#line 1631
    (&kernelContext_30)->frame_0 = frame_4;

#line 1631
    (&kernelContext_30)->field_0 = field_4;

#line 1631
    (&kernelContext_30)->grassCard_0 = grassCard_4;

#line 1631
    (&kernelContext_30)->grassCardSampler_0 = grassCardSampler_4;

#line 1631
    (&kernelContext_30)->cluster_lights_0 = cluster_lights_4;

#line 1631
    (&kernelContext_30)->lights_0 = lights_4;

#line 1631
    (&kernelContext_30)->shadow_atlas_0 = shadow_atlas_4;

#line 1631
    (&kernelContext_30)->shadow_sampler_0 = shadow_sampler_4;

#line 1631
    (&kernelContext_30)->grassGround_0 = grassGround_4;

#line 1631
    (&kernelContext_30)->wind_0 = wind_4;

#line 1631
    (&kernelContext_30)->windDirectionLayer_0 = windDirectionLayer_4;

#line 1631
    (&kernelContext_30)->windSampler_0 = windSampler_4;

#line 1631
    (&kernelContext_30)->windIntensityLayer_0 = windIntensityLayer_4;

#line 1631
    (&kernelContext_30)->grassCells_0 = grassCells_4;

#line 1631
    uint _S173 = grass_shell_count_0(&kernelContext_30);

#line 1718
    uint _S174 = _S173 - 1U;
    uint quad_0 = index_5 / 6U;

#line 1719
    GrassShellVertex_0 _S175 = grass_sheet_vertex_0((&kernelContext_30)->tile_1->tile_0.xy + (float2(float(quad_0 % 16U), float(quad_0 / 16U)) + GRASS_CARD_CORNERS_0[index_5 % 6U]) * float2(((&kernelContext_30)->tile_1->tile_0.z / 16.0f)) , (&kernelContext_30)->field_0->layers_0[_S174 - min(instance_id_1, _S174)].x, float4(0.0f, 1.0f, 0.0f, 0.0f), (&kernelContext_30)->field_0->layers_0[_S174 - min(instance_id_1, _S174)].y, &kernelContext_30);

#line 1719
    thread shellVertexMain_Result_0 _S176;

#line 1719
    (&_S176)->position_6 = _S175.position_2;

#line 1719
    (&_S176)->world_position_11 = _S175.world_position_7;

#line 1719
    (&_S176)->rest_3 = _S175.rest_0;

#line 1719
    (&_S176)->normal_8 = _S175.normal_3;

#line 1719
    (&_S176)->footprint_2 = _S175.footprint_0;

#line 1719
    (&_S176)->occlusion_5 = _S175.occlusion_1;

#line 1719
    (&_S176)->fin_3 = _S175.fin_0;

#line 1719
    return _S176;
}


#line 1719
struct finVertexMain_Result_0
{
    float4 position_7 [[position]];
    float3 world_position_12 [[user(WORLD_POSITION)]];
    float3 rest_4 [[user(REST_POSITION)]];
    float3 normal_9 [[user(NORMAL)]];
    float footprint_3 [[user(FOOTPRINT)]];
    float occlusion_6 [[user(OCCLUSION)]];
    float4 fin_4 [[user(FIN)]];
};


#line 1719
[[vertex]] finVertexMain_Result_0 finVertexMain(uint index_6 [[vertex_id]], GrassTile_0 constant* tile_13 [[buffer(2)]], GrassParams_0 constant* grass_5 [[buffer(1)]], GrassInstance_natural_0 device* instances_5 [[buffer(3)]], GrassBlade_natural_0 device* blades_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], GrassField_0 constant* field_5 [[buffer(7)]], texture2d_array<float, access::sample> grassCard_5 [[texture(0)]], sampler grassCardSampler_5 [[sampler(0)]], uint device* cluster_lights_5 [[buffer(6)]], GpuLight_natural_0 device* lights_5 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> grassGround_5 [[texture(2)]], WindParams_0 constant* wind_5 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_5 [[texture(3)]], sampler windSampler_5 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_5 [[texture(4)]], GrassInstance_natural_0 device* grassCells_5 [[buffer(8)]])
{

#line 1719
    thread KernelContext_0 kernelContext_31;

#line 1719
    (&kernelContext_31)->tile_1 = tile_13;

#line 1719
    (&kernelContext_31)->grass_0 = grass_5;

#line 1719
    (&kernelContext_31)->instances_0 = instances_5;

#line 1719
    (&kernelContext_31)->blades_0 = blades_5;

#line 1719
    (&kernelContext_31)->frame_0 = frame_5;

#line 1719
    (&kernelContext_31)->field_0 = field_5;

#line 1719
    (&kernelContext_31)->grassCard_0 = grassCard_5;

#line 1719
    (&kernelContext_31)->grassCardSampler_0 = grassCardSampler_5;

#line 1719
    (&kernelContext_31)->cluster_lights_0 = cluster_lights_5;

#line 1719
    (&kernelContext_31)->lights_0 = lights_5;

#line 1719
    (&kernelContext_31)->shadow_atlas_0 = shadow_atlas_5;

#line 1719
    (&kernelContext_31)->shadow_sampler_0 = shadow_sampler_5;

#line 1719
    (&kernelContext_31)->grassGround_0 = grassGround_5;

#line 1719
    (&kernelContext_31)->wind_0 = wind_5;

#line 1719
    (&kernelContext_31)->windDirectionLayer_0 = windDirectionLayer_5;

#line 1719
    (&kernelContext_31)->windSampler_0 = windSampler_5;

#line 1719
    (&kernelContext_31)->windIntensityLayer_0 = windIntensityLayer_5;

#line 1719
    (&kernelContext_31)->grassCells_0 = grassCells_5;

#line 1756
    uint quad_1 = index_6 / 6U;
    uint _S177 = index_6 % 6U;
    uint _S178 = max(field_5->tiles_0.z / 4U, 1U);

    uint _S179 = quad_1 / (_S178 * 64U);

#line 1760
    uint _S180 = min(_S179, 1U);
    uint fin_line_0 = quad_1 / 64U % _S178;



    float band_3 = 4.0f * (&kernelContext_31)->field_0->origin_0.w;
    float across_1 = (float(fin_line_0) + 0.5f) * band_3;
    float stride_0 = (&kernelContext_31)->tile_1->tile_0.z / 16.0f;
    float _S181 = float(quad_1 / 4U % 16U);

#line 1768
    float along_4 = (_S181 + GRASS_CARD_CORNERS_0[_S177].x) * stride_0;
    float middle_along_0 = (_S181 + 0.5f) * stride_0;
    float share_2 = (float(quad_1 % 4U) + GRASS_CARD_CORNERS_0[_S177].y) / 4.0f;



    bool along_z_1 = _S180 == 0U;
    float2 _S182 = (&kernelContext_31)->tile_1->tile_0.xy;

#line 1775
    float2 side_1;

#line 1775
    if(along_z_1)
    {

#line 1775
        side_1 = float2(across_1, along_4);

#line 1775
    }
    else
    {

#line 1775
        side_1 = float2(along_4, across_1);

#line 1775
    }

#line 1775
    float2 rest_5 = _S182 + side_1;

    float2 _S183 = (&kernelContext_31)->tile_1->tile_0.xy;

#line 1777
    if(along_z_1)
    {

#line 1777
        side_1 = float2(across_1, middle_along_0);

#line 1777
    }
    else
    {

#line 1777
        side_1 = float2(middle_along_0, across_1);

#line 1777
    }

#line 1777
    float2 middle_0 = _S183 + side_1;
    if(along_z_1)
    {

#line 1778
        side_1 = float2(0.5f * band_3, 0.0f);

#line 1778
    }
    else
    {

#line 1778
        side_1 = float2(0.0f, 0.5f * band_3);

#line 1778
    }

#line 1778
    GrassGround_0 _S184 = grass_ground_under_0(middle_0, &kernelContext_31);

#line 1778
    GrassGround_0 _S185 = grass_ground_under_0(middle_0 - side_1, &kernelContext_31);

#line 1778
    GrassGround_0 _S186 = grass_ground_under_0(middle_0 + side_1, &kernelContext_31);

#line 1784
    float3 eye_0 = (&kernelContext_31)->frame_0->camera_position_0.xyz - float3(middle_0.x, _S184.height_0 + 0.5f * (&kernelContext_31)->field_0->stack_0.x, middle_0.y);
    float3 view_0 = eye_0 / float3(max(length(eye_0), 9.99999997475242708e-07f)) ;
    float facing_near_0 = dot(_S185.normal_2, view_0);
    float facing_far_0 = dot(_S186.normal_2, view_0);

#line 1787
    float graze_0;

    if((facing_near_0 * facing_far_0) <= 0.0f)
    {

#line 1789
        graze_0 = 0.0f;

#line 1789
    }
    else
    {

#line 1789
        graze_0 = min(abs(facing_near_0), abs(facing_far_0));

#line 1789
    }

    float fade_2 = saturate((0.5f - graze_0) / 0.19999998807907104f);


    if(along_z_1)
    {

#line 1794
        graze_0 = (&kernelContext_31)->tile_1->tile_0.x;

#line 1794
    }
    else
    {

#line 1794
        graze_0 = (&kernelContext_31)->tile_1->tile_0.y;

#line 1794
    }

    float4 _S187 = float4(float(_S180) + 1.0f, fade_2, graze_0 + across_1, 0.0f);

#line 1796
    float _S188 = grass_occlusion_at_0(share_2, &kernelContext_31);

#line 1796
    GrassShellVertex_0 _S189 = grass_sheet_vertex_0(rest_5, share_2, _S187, _S188, &kernelContext_31);

#line 1795
    thread GrassShellVertex_0 output_4 = _S189;

#line 1795
    bool _S190;

    if(fade_2 <= 0.0f)
    {

#line 1797
        _S190 = true;

#line 1797
    }
    else
    {

#line 1797
        _S190 = ((&kernelContext_31)->field_0->stack_0.z) <= 0.0f;

#line 1797
    }

#line 1797
    if(_S190)
    {


        (&output_4)->position_2 = float4(0.0f, 0.0f, -1.0f, 1.0f);

#line 1797
    }

#line 1803
    GrassShellVertex_0 _S191 = output_4;

#line 1803
    thread finVertexMain_Result_0 _S192;

#line 1803
    (&_S192)->position_7 = _S191.position_2;

#line 1803
    (&_S192)->world_position_12 = _S191.world_position_7;

#line 1803
    (&_S192)->rest_4 = _S191.rest_0;

#line 1803
    (&_S192)->normal_9 = _S191.normal_3;

#line 1803
    (&_S192)->footprint_3 = _S191.footprint_0;

#line 1803
    (&_S192)->occlusion_6 = _S191.occlusion_1;

#line 1803
    (&_S192)->fin_4 = _S191.fin_0;

#line 1803
    return _S192;
}

