#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 562 "shaders/grass.slang"
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 516
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 537
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 550
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 659
constant array<float2, int(6)> GRASS_CARD_CORNERS_0 = { float2(0.0f, 0.0f), float2(1.0f, 0.0f), float2(0.0f, 1.0f), float2(1.0f, 0.0f), float2(1.0f, 1.0f), float2(0.0f, 1.0f) };

#line 634
float grassCardLevel_0(float pixels_0)
{
    float _S1 = 64.0f / max(pixels_0, 0.00009999999747379f);

#line 636
    float level_0 = 0.0f;

#line 636
    uint step_0 = 1U;

    for(;;)
    {

#line 638
        if(step_0 < 7U)
        {
        }
        else
        {

#line 638
            break;
        }
        uint _S2 = step_0 - 1U;

#line 640
        float lower_0 = float(1U << _S2);
        if(_S1 >= lower_0)
        {

#line 641
            level_0 = float(_S2) + min((_S1 - lower_0) / lower_0, 1.0f);

#line 641
        }

#line 638
        step_0 = step_0 + 1U;

#line 638
    }

#line 654
    return min(floor(level_0 + 0.5f), 6.0f);
}


#line 403
struct GrassTile_0
{
    float4 tile_0;
    uint4 slot_0;
};


#line 388
struct GrassParams_0
{
    uint4 limits_0;
    float4 screen_0;
};


#line 388
struct GrassInstance_natural_0
{
    packed_float4 root_0;
    packed_float4 facing_0;
    packed_float4 lean_0;
    packed_float4 ground_0;
    packed_uint4 lanes_0;
};


#line 388
struct GrassBlade_natural_0
{
    packed_float4 root_color_0;
    packed_float4 tip_color_0;
    packed_float4 size_0;
};


#line 388
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 388
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 388
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(14)> data_2;
};


#line 83
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


#line 1266
struct GpuLight_natural_0
{
    packed_float4 position_0;
    packed_float4 color_0;
    packed_float4 direction_0;
    packed_float4 tangent_0;
    uint kind_0;
    float cos_inner_0;
    uint shadow_tile_0;
    uint flags_0;
};


#line 1266
struct KernelContext_0
{
    GrassTile_0 constant* tile_1;
    GrassParams_0 constant* grass_0;
    GrassInstance_natural_0 device* instances_0;
    GrassBlade_natural_0 device* blades_0;
    FrameUniforms_natural_0 constant* frame_0;
    texture2d_array<float, access::sample> grassCard_0;
    sampler grassCardSampler_0;
    uint device* cluster_lights_0;
    GpuLight_natural_0 device* lights_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
};


#line 1148
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S3 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S4 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S5 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S6 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 1158
    uint _S7 = uint(pixel_0.x) / _S6;

#line 1158
    uint _S8 = min(_S7, _S3 - 1U);
    uint _S9 = uint(pixel_0.y) / _S6;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1169
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S5 - 1U))) * _S4 + min(_S9, _S4 - 1U)) * _S3 + _S8;
}


#line 1126
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

#line 1145
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 692
float4 atlas_rect_0(uint tile_2, KernelContext_0 thread* kernelContext_1)
{
    return kernelContext_1->frame_0->shadow_atlas_rect_0[tile_2];
}


#line 692
float4 atlas_rect_1(uint tile_3, KernelContext_0 thread* kernelContext_2)
{
    return kernelContext_2->frame_0->shadow_atlas_rect_0[tile_3];
}


#line 707
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 702
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_3)
{
    return rect_1.x / kernelContext_3->frame_0->shadow_params_0.x;
}


#line 669
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 681
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_4)
{

#line 681
    uint _S10;

    if(uint(pixel_1.x) < (kernelContext_4->frame_0->shadow_filter_0.z))
    {

#line 683
        _S10 = kernelContext_4->frame_0->shadow_filter_0.x;

#line 683
    }
    else
    {

#line 683
        _S10 = kernelContext_4->frame_0->shadow_filter_0.y;

#line 683
    }

#line 683
    return _S10;
}


#line 697
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_5)
{
    return kernelContext_5->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 697
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_6)
{
    return kernelContext_6->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 687
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 712
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_7)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S11 = spoke_0.x;

#line 717
    float _S12 = rotation_0.x;

#line 717
    float _S13 = spoke_0.y;

#line 717
    float _S14 = rotation_0.y;


    float _S15 = ((kernelContext_7->shadow_atlas_0).sample_compare((kernelContext_7->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S11 * _S12 - _S13 * _S14, _S11 * _S14 + _S13 * _S12) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 720
    return _S15;
}


#line 761
float tile_box_pcf_0(uint tile_4, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_8)
{

#line 761
    float4 _S16 = atlas_rect_1(tile_4, kernelContext_8);


    if(atlas_rect_is_empty_0(_S16))
    {
        return 1.0f;
    }

#line 766
    float2 _S17 = atlas_step_1(_S16, kernelContext_8);

#line 766
    int y_0 = int(-1);

#line 766
    float visibility_0 = 0.0f;

#line 771
    for(;;)
    {

#line 771
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 771
            break;
        }

#line 771
        int x_0 = int(-1);

        for(;;)
        {

#line 773
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 773
                break;
            }

#line 773
            float _S18 = tile_tap_0(_S16, _S17, tile_uv_2, float2(float(x_0), float(y_0)), float2(1.0f, 0.0f), reference_1, kernelContext_8);

            float visibility_1 = visibility_0 + _S18;

#line 773
            x_0 = x_0 + int(1);

#line 773
            visibility_0 = visibility_1;

#line 773
        }

#line 771
        y_0 = y_0 + int(1);

#line 771
    }

#line 779
    return visibility_0 / 9.0f;
}


#line 675
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 723
float tile_pcf_0(uint tile_5, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_9)
{
    float2 _S19 = shadow_rotation_0(pixel_3);

#line 725
    float4 _S20 = atlas_rect_1(tile_5, kernelContext_9);

    if(atlas_rect_is_empty_0(_S20))
    {
        return 1.0f;
    }

#line 729
    float2 _S21 = atlas_step_1(_S20, kernelContext_9);

#line 729
    uint spot_0 = 0U;

#line 729
    float probe_0 = 0.0f;

#line 734
    for(;;)
    {

#line 734
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 734
            break;
        }

#line 734
        float _S22 = tile_tap_0(_S20, _S21, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S19, reference_2, kernelContext_9);

        float probe_1 = probe_0 + _S22;

#line 734
        spot_0 = spot_0 + 1U;

#line 734
        probe_0 = probe_1;

#line 734
    }

#line 743
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 749
    uint index_0 = 0U;

#line 749
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 753
        if(index_0 < 32U)
        {
        }
        else
        {

#line 753
            break;
        }

#line 753
        float _S23 = tile_tap_0(_S20, _S21, tile_uv_3, SHADOW_DISC_0[index_0] * float2(radius_2) , _S19, reference_2, kernelContext_9);

        float visibility_3 = visibility_2 + _S23;

#line 753
        index_0 = index_0 + 1U;

#line 753
        visibility_2 = visibility_3;

#line 753
    }

#line 758
    return visibility_2 / 32.0f;
}


#line 782
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_10)
{
    float2 texel_0 = kernelContext_10->frame_0->shadow_params_0.xy;

#line 784
    float4 _S24 = atlas_rect_0(cascade_0, kernelContext_10);

#line 784
    float2 _S25 = atlas_step_0(_S24, kernelContext_10);


    float2 _S26 = float2(0.5f, 0.5f) * _S25;


    float2 _S27 = float2(1.0f, 1.0f);

#line 790
    float2 _S28 = _S27 / texel_0;

#line 790
    uint index_1 = 0U;

#line 790
    float sum_0 = 0.0f;

#line 790
    float found_0 = 0.0f;



    for(;;)
    {

#line 794
        if(index_1 < 16U)
        {
        }
        else
        {

#line 794
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_1] * float2(8.0f) ;
        float _S29 = spoke_1.x;

#line 797
        float _S30 = rotation_1.x;

#line 797
        float _S31 = spoke_1.y;

#line 797
        float _S32 = rotation_1.y;

#line 805
        int3 _S33 = int3(int2(min(atlas_uv_0(_S24, clamp(tile_uv_4 + float2(_S29 * _S30 - _S31 * _S32, _S29 * _S32 + _S31 * _S30) * _S25, _S26, float2(1.0f)  - _S26)) * _S28, _S28 - _S27)), int(0));

#line 805
        float depth_1 = ((kernelContext_10->shadow_atlas_0).read(vec<uint,2>(((_S33)).xy), uint(((_S33)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 809
            sum_0 = sum_0 + depth_1;

#line 809
            found_0 = found_1;

#line 806
        }

#line 794
        index_1 = index_1 + 1U;

#line 794
    }

#line 813
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 824
    float _S34 = 2.0f * kernelContext_10->frame_0->cascade_far_0[cascade_0];

#line 824
    float separation_0 = (sum_0 / found_0 - reference_3) * (_S34 + 40.0f);

#line 824
    float _S35 = tile_texels_0(_S24, kernelContext_10);

    return clamp(separation_0 * 0.01999999955296516f / (_S34 / _S35), 2.0f, 8.0f);
}



float cascade_visibility_0(uint cascade_1, float3 world_position_0, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_11)
{

#line 832
    float4 _S36 = atlas_rect_0(cascade_1, kernelContext_11);

#line 866
    if(atlas_rect_is_empty_0(_S36))
    {


        return 1.0f;
    }
    float _S37 = 2.0f * kernelContext_11->frame_0->cascade_far_0[cascade_1];

#line 872
    float _S38 = tile_texels_0(_S36, kernelContext_11);

#line 872
    float texel_world_0 = _S37 / _S38;

#line 879
    float4 clip_0 = (((float4(world_position_0 + geometric_normal_1 * float3((texel_world_0 * kernelContext_11->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_11->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 883
    bool _S39;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 884
        _S39 = true;

#line 884
    }
    else
    {

#line 884
        _S39 = (ndc_0.z) <= 0.0f;

#line 884
    }

#line 884
    if(_S39)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 894
    uint _S40 = shadow_filter_mode_0(pixel_4, kernelContext_11);

#line 911
    if(_S40 == 2U)
    {

#line 911
        float _S41 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_11);

        return _S41;
    }
    if(_S40 == 1U)
    {

#line 915
        float _S42 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_11);



        return _S42;
    }

    float _S43 = ndc_0.z;

#line 922
    float _S44 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S43, shadow_rotation_0(pixel_4), kernelContext_11);

#line 922
    float _S45 = tile_pcf_0(cascade_1, tile_uv_5, _S43, pixel_4, _S44, kernelContext_11);
    return _S45;
}

float sun_visibility_0(float3 world_position_1, float3 to_light_3, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_12)
{
    uint cascade_2;

#line 928
    bool covered_0;

#line 937
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 949
    float eye_distance_0 = length(world_position_1 - kernelContext_12->frame_0->camera_position_0.xyz);

#line 949
    uint index_2 = 0U;

#line 957
    for(;;)
    {

#line 957
        if(index_2 < 2U)
        {
        }
        else
        {

#line 957
            covered_0 = false;

#line 957
            cascade_2 = 1U;

#line 957
            break;
        }
        if(eye_distance_0 < kernelContext_12->frame_0->cascade_far_0[index_2])
        {

#line 959
            covered_0 = true;

#line 959
            cascade_2 = index_2;



            break;
        }

#line 957
        index_2 = index_2 + 1U;

#line 957
    }

#line 966
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 966
    }

#line 966
    float _S46 = cascade_visibility_0(cascade_2, world_position_1, to_light_3, geometric_normal_2, pixel_5, kernelContext_12);

#line 973
    uint _S47 = cascade_2 + 1U;

#line 973
    if(_S47 >= 2U)
    {



        return _S46;
    }

#line 986
    float band_0 = kernelContext_12->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_12->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S46;
    }

#line 994
    float _S48 = cascade_visibility_0(_S47, world_position_1, to_light_3, geometric_normal_2, pixel_5, kernelContext_12);

#line 1005
    return mix(_S46, _S48, blend_0);
}


#line 1094
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S49 = axis_1.x;

#line 1097
    float _S50 = axis_1.y;

#line 1097
    bool _S51;

#line 1097
    if(_S49 >= _S50)
    {

#line 1097
        _S51 = _S49 >= (axis_1.z);

#line 1097
    }
    else
    {

#line 1097
        _S51 = false;

#line 1097
    }

#line 1097
    uint _S52;

#line 1097
    if(_S51)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1099
            _S52 = 0U;

#line 1099
        }
        else
        {

#line 1099
            _S52 = 1U;

#line 1099
        }

#line 1099
        return _S52;
    }
    if(_S50 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1103
            _S52 = 2U;

#line 1103
        }
        else
        {

#line 1103
            _S52 = 3U;

#line 1103
        }

#line 1103
        return _S52;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1105
        _S52 = 4U;

#line 1105
    }
    else
    {

#line 1105
        _S52 = 5U;

#line 1105
    }

#line 1105
    return _S52;
}


#line 664
uint light_tile_0(uint tile_6)
{
    return 2U + tile_6;
}


#line 1008
float punctual_visibility_0(uint tile_7, float3 world_position_2, float3 to_light_4, float n_dot_l_1, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_13)
{

    uint atlas_0 = light_tile_0(tile_7);

#line 1011
    float4 _S53 = atlas_rect_0(atlas_0, kernelContext_13);

    if(atlas_rect_is_empty_0(_S53))
    {


        return 1.0f;
    }

#line 1017
    float _S54 = tile_texels_0(_S53, kernelContext_13);

    float texel_world_1 = map_world_0 / _S54;

#line 1029
    float4 clip_1 = (((float4(world_position_2 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(0)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(0)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(0)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(0)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(1)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(1)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(1)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(1)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(2)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(2)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(2)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(2)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(3)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(3)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(3)], (&kernelContext_13->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(3)]))));

#line 1036
    float _S55 = clip_1.w;

#line 1036
    if(_S55 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S55) ;

#line 1040
    bool _S56;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1041
        _S56 = true;

#line 1041
    }
    else
    {

#line 1041
        _S56 = (ndc_1.z) <= 0.0f;

#line 1041
    }

#line 1041
    if(_S56)
    {

#line 1041
        _S56 = true;

#line 1041
    }
    else
    {

#line 1041
        _S56 = (ndc_1.z) > 1.0f;

#line 1041
    }

#line 1041
    if(_S56)
    {

#line 1048
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 1053
    uint _S57 = shadow_filter_mode_0(pixel_6, kernelContext_13);

#line 1062
    if(_S57 == 2U)
    {

#line 1062
        float _S58 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_13);

        return _S58;
    }

#line 1064
    float _S59 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_13);

    return _S59;
}


#line 1108
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_0, float3 world_position_3, float3 to_light_5, float n_dot_l_2, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_14)
{

    if(n_dot_l_2 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_3 - (float4(light_0->position_0) ).xyz;

#line 1116
    float _S60 = punctual_visibility_0(base_0 + point_face_0(from_light_1), world_position_3, to_light_5, n_dot_l_2, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_14);

#line 1122
    return _S60;
}


#line 1069
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_8, float3 world_position_4, float3 to_light_6, float n_dot_l_3, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_15)
{

    if(n_dot_l_3 <= 0.0f)
    {


        return 1.0f;
    }

#line 1076
    float4 _S61 = float4(light_1->direction_0) ;

#line 1083
    float cos_outer_1 = _S61.w;

#line 1083
    float _S62 = punctual_visibility_0(tile_8, world_position_4, to_light_6, n_dot_l_3, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_4 - (float4(light_1->position_0) ).xyz, normalize(_S61.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_15);

#line 1090
    return _S62;
}


#line 1090
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1090
struct pixelInput_0
{
    float3 world_position_5 [[user(WORLD_POSITION)]];
    float3 normal_0 [[user(NORMAL)]];
    float3 color_1 [[user(COLOR)]];
    float2 uv_0 [[user(TEXCOORD)]];
    float level_1 [[user(CARD_LEVEL)]];
};


#line 1241
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S63 [[stage_in]], float4 position_1 [[position]], GrassTile_0 constant* tile_9 [[buffer(2)]], GrassParams_0 constant* grass_1 [[buffer(1)]], GrassInstance_natural_0 device* instances_1 [[buffer(3)]], GrassBlade_natural_0 device* blades_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], texture2d_array<float, access::sample> grassCard_1 [[texture(0)]], sampler grassCardSampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(6)]], GpuLight_natural_0 device* lights_1 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]])
{

#line 1241
    thread KernelContext_0 kernelContext_16;

#line 1241
    (&kernelContext_16)->tile_1 = tile_9;

#line 1241
    (&kernelContext_16)->grass_0 = grass_1;

#line 1241
    (&kernelContext_16)->instances_0 = instances_1;

#line 1241
    (&kernelContext_16)->blades_0 = blades_1;

#line 1241
    (&kernelContext_16)->frame_0 = frame_1;

#line 1241
    (&kernelContext_16)->grassCard_0 = grassCard_1;

#line 1241
    (&kernelContext_16)->grassCardSampler_0 = grassCardSampler_1;

#line 1241
    (&kernelContext_16)->cluster_lights_0 = cluster_lights_1;

#line 1241
    (&kernelContext_16)->lights_0 = lights_1;

#line 1241
    (&kernelContext_16)->shadow_atlas_0 = shadow_atlas_1;

#line 1241
    (&kernelContext_16)->shadow_sampler_0 = shadow_sampler_1;

#line 1247
    float3 _S64 = float3(_S63.uv_0.x, 1.0f - _S63.uv_0.y, 0.0f);

    if((((grassCard_1).sample((grassCardSampler_1), ((_S64)).xy, uint(((_S64)).z), level((_S63.level_1)))).x) < 0.5f)
    {
        discard_fragment();

#line 1249
    }

#line 1255
    float3 normal_1 = normalize(_S63.normal_0);

#line 1265
    float2 _S65 = position_1.xy;

#line 1265
    uint _S66 = froxel_of_0(_S65, (((float4(_S63.world_position_5, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_16)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_16)->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w, &kernelContext_16);

#line 1265
    uint base_1 = _S66 * 17U;
    uint _S67 = min((&kernelContext_16)->cluster_lights_0[base_1], 16U);

    float3 _S68 = float3(0.0f, 0.0f, 0.0f);

#line 1268
    uint slot_1 = 0U;

#line 1268
    float3 direct_0 = _S68;
    for(;;)
    {

#line 1269
        if(slot_1 < _S67)
        {
        }
        else
        {

#line 1269
            break;
        }

#line 1269
        thread GpuLight_natural_0 _S69 = (&kernelContext_16)->lights_0[(&kernelContext_16)->cluster_lights_0[base_1 + 1U + slot_1]];

#line 1269
        uint _S70 = (&_S69)->kind_0;


        if(((&_S69)->kind_0) == 3U)
        {



            slot_1 = slot_1 + 1U;

#line 1269
            continue;
        }

#line 1282
        bool _S71 = _S70 == 0U;

#line 1282
        float3 to_light_7;

#line 1282
        float reach_0;

#line 1282
        if(_S71)
        {

#line 1282
            to_light_7 = normalize((float4((&_S69)->direction_0) ).xyz);

#line 1282
            reach_0 = 1.0f;

#line 1282
        }
        else
        {

#line 1282
            float4 _S72 = float4((&_S69)->position_0) ;

#line 1289
            float3 offset_0 = _S72.xyz - _S63.world_position_5;
            float distance_2 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_2, _S72.w);
            if(_S70 == 2U)
            {

#line 1293
                float4 _S73 = float4((&_S69)->direction_0) ;

#line 1293
                reach_0 = reach_1 * spot_cone_0(to_light_8, _S73.xyz, _S73.w, (&_S69)->cos_inner_0);

#line 1293
            }
            else
            {

#line 1293
                reach_0 = reach_1;

#line 1293
            }

#line 1293
            to_light_7 = to_light_8;

#line 1282
        }

#line 1300
        float n_dot_l_4 = dot(normal_1, to_light_7);

#line 1300
        float reach_2;
        if(_S71)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 1304
            float _S74 = sun_visibility_0(_S63.world_position_5, to_light_7, n_dot_l_4, normal_1, _S65, &sun_cascade_0, &sun_fade_0, &kernelContext_16);

#line 1304
            reach_2 = _S74;

#line 1301
        }
        else
        {

#line 1308
            if(_S70 == 1U)
            {

#line 1308
                uint _S75 = (&_S69)->shadow_tile_0;

                if(((&_S69)->shadow_tile_0) <= 8U)
                {

#line 1310
                    float _S76 = point_visibility_0(&_S69, _S75, _S63.world_position_5, to_light_7, n_dot_l_4, normal_1, _S65, &kernelContext_16);

#line 1310
                    reach_2 = reach_0 * _S76;

#line 1310
                }
                else
                {

#line 1310
                    reach_2 = reach_0;

#line 1310
                }

#line 1308
            }
            else
            {

#line 1308
                uint _S77 = (&_S69)->shadow_tile_0;

#line 1316
                if(((&_S69)->shadow_tile_0) < 14U)
                {

#line 1316
                    float _S78 = spot_visibility_0(&_S69, _S77, _S63.world_position_5, to_light_7, n_dot_l_4, normal_1, _S65, &kernelContext_16);

#line 1316
                    reach_2 = reach_0 * _S78;

#line 1316
                }
                else
                {

#line 1316
                    reach_2 = reach_0;

#line 1316
                }

#line 1308
            }

#line 1301
        }

#line 1301
        direct_0 = direct_0 + (float4((&_S69)->color_0) ).xyz * float3((max(n_dot_l_4, 0.0f) * reach_2)) ;

#line 1269
        slot_1 = slot_1 + 1U;

#line 1269
    }

#line 1269
    pixelOutput_0 _S79 = { float4(_S63.color_1 * (direct_0 + (&kernelContext_16)->frame_0->ambient_0.xyz), 1.0f) };

#line 1326
    return _S79;
}


#line 1326
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float3 world_position_6 [[user(WORLD_POSITION)]];
    float3 normal_2 [[user(NORMAL)]];
    float3 color_2 [[user(COLOR)]];
    float2 uv_1 [[user(TEXCOORD)]];
    float level_2 [[user(CARD_LEVEL)]];
};


#line 1174
struct GrassVertex_0
{
    float4 position_3;
    float3 world_position_7;
    float3 normal_3;
    float3 color_3;
    float2 uv_2;
    float level_3;
};


#line 1174
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], uint instance_id_0 [[instance_id]], GrassTile_0 constant* tile_10 [[buffer(2)]], GrassParams_0 constant* grass_2 [[buffer(1)]], GrassInstance_natural_0 device* instances_2 [[buffer(3)]], GrassBlade_natural_0 device* blades_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], texture2d_array<float, access::sample> grassCard_2 [[texture(0)]], sampler grassCardSampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(6)]], GpuLight_natural_0 device* lights_2 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]])
{

#line 1174
    thread KernelContext_0 kernelContext_17;

#line 1174
    (&kernelContext_17)->tile_1 = tile_10;

#line 1174
    (&kernelContext_17)->grass_0 = grass_2;

#line 1174
    (&kernelContext_17)->instances_0 = instances_2;

#line 1174
    (&kernelContext_17)->blades_0 = blades_2;

#line 1174
    (&kernelContext_17)->frame_0 = frame_2;

#line 1174
    (&kernelContext_17)->grassCard_0 = grassCard_2;

#line 1174
    (&kernelContext_17)->grassCardSampler_0 = grassCardSampler_2;

#line 1174
    (&kernelContext_17)->cluster_lights_0 = cluster_lights_2;

#line 1174
    (&kernelContext_17)->lights_0 = lights_2;

#line 1174
    (&kernelContext_17)->shadow_atlas_0 = shadow_atlas_2;

#line 1174
    (&kernelContext_17)->shadow_sampler_0 = shadow_sampler_2;

#line 1200
    GrassInstance_natural_0 blade_0 = instances_2[tile_10->slot_0.x * grass_2->limits_0.x + instance_id_0];
    GrassBlade_natural_0 row_0 = blades_2[min((uint4(blade_0.lanes_0) ).y, max(grass_2->limits_0.y, 1U) - 1U)];


    uint _S80 = index_3 % 6U;

#line 1204
    float4 _S81 = float4(blade_0.facing_0) ;

#line 1209
    float2 facing_1 = _S81.xy;

#line 1209
    float2 across_0;
    if((index_3 / 6U) == 0U)
    {

#line 1210
        across_0 = facing_1;

#line 1210
    }
    else
    {

#line 1210
        across_0 = float2(- facing_1.y, facing_1.x);

#line 1210
    }

    float _S82 = _S81.z;

#line 1217
    float _S83 = GRASS_CARD_CORNERS_0[_S80].y;

#line 1217
    float4 _S84 = float4(blade_0.root_0) ;
    float3 world_0 = _S84.xyz + float3(across_0.x, 0.0f, across_0.y) * float3((_S82 * (GRASS_CARD_CORNERS_0[_S80].x * 2.0f - 1.0f)))  + float3(0.0f, _S84.w * _S83, 0.0f) + (float4(blade_0.lean_0) ).xyz * float3((_S83 * _S83)) ;

    thread GrassVertex_0 output_1;
    float4 _S85 = (((float4(world_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_17)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_17)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));

#line 1221
    (&output_1)->position_3 = _S85;
    (&output_1)->world_position_7 = world_0;
    (&output_1)->normal_3 = (float4(blade_0.ground_0) ).xyz;

#line 1228
    (&output_1)->color_3 = mix((float4(row_0.root_color_0) ).xyz, (float4(row_0.tip_color_0) ).xyz, float3(_S83) ) * float3((1.0f - 0.18000000715255737f * _S81.w)) ;
    (&output_1)->uv_2 = GRASS_CARD_CORNERS_0[_S80];

#line 1234
    (&output_1)->level_3 = grassCardLevel_0(2.0f * _S82 * (&kernelContext_17)->grass_0->screen_0.x / max(abs(_S85.w), 0.00009999999747379f));

    GrassVertex_0 _S86 = output_1;

#line 1236
    thread vertexMain_Result_0 _S87;

#line 1236
    (&_S87)->position_2 = _S86.position_3;

#line 1236
    (&_S87)->world_position_6 = _S86.world_position_7;

#line 1236
    (&_S87)->normal_2 = _S86.normal_3;

#line 1236
    (&_S87)->color_2 = _S86.color_3;

#line 1236
    (&_S87)->uv_1 = _S86.uv_2;

#line 1236
    (&_S87)->level_2 = _S86.level_3;

#line 1236
    return _S87;
}

