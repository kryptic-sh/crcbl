#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 111 "shaders/volumetric.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f } };

#line 106
constant array<float, int(8)> FOG_KERNEL_0 = { { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f } };

#line 644
constant array<float2, int(32)> SHADOW_DISC_0 = { { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) } };

#line 671
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { { 0U, 23U, 25U, 27U, 29U } };

constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) } };

#line 684
constant array<uint, int(16)> SHADOW_DITHER_0 = { { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U } };

#line 283
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 283
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 283
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(14)> data_2;
};


#line 192
struct VolumetricParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inverse_view_proj_0;
    float4 eye_0;
    float4 depth_row_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sun_direction_0;
    float4 sun_radiance_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
    uint grid_x_0;
    uint grid_y_0;
    uint slices_0;
    uint tile_pixels_0;
    uint viewport_x_0;
    uint viewport_y_0;
    uint froxel_count_0;
    uint pad0_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0 light_view_proj_0;
    array<float4, int(16)> shadow_atlas_rect_0;
    float4 aerial_params_0;
};


#line 332
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


#line 292
struct KernelContext_0
{
    VolumetricParams_natural_0 constant* params_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    uint device* cluster_lights_0;
    GpuLight_natural_0 device* lights_0;
    packed_float4 device* lighting_0;
    packed_float4 device* volumetrics_0;
};


#line 394
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 430
void volumetric_tile_ray_0(uint tile_x_0, uint tile_y_0, float3 thread* near_point_0, float thread* near_depth_0, KernelContext_0 thread* kernelContext_1)
{

    float2 pixel_0 = (float2(float(tile_x_0), float(tile_y_0)) + float2(0.5f) ) * float2(float(kernelContext_1->params_0->tile_pixels_0)) ;

#line 433
    float3 _S1 = volumetric_unproject_0(float2(pixel_0.x / float(max(kernelContext_1->params_0->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - pixel_0.y / float(max(kernelContext_1->params_0->viewport_y_0, 1U)) * 2.0f), 1.0f, kernelContext_1);



    *near_point_0 = _S1;
    *near_depth_0 = max(dot(kernelContext_1->params_0->depth_row_0, float4(_S1, 1.0f)), 9.99999997475242708e-07f);
    return;
}


#line 409
float volumetric_slice_start_0(uint index_0)
{

#line 409
    uint step_0 = 0U;

#line 409
    float start_0 = 0.10000000149011612f;


    for(;;)
    {

#line 412
        if(step_0 < index_0)
        {
        }
        else
        {

#line 412
            break;
        }
        float start_1 = start_0 * 1.46779930591583252f;

#line 412
        step_0 = step_0 + 1U;

#line 412
        start_0 = start_1;

#line 412
    }



    return start_0;
}


#line 693
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}



float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_2)
{
    return kernelContext_2->params_0->shadow_atlas_rect_0[tile_0];
}


#line 718
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 708
float2 atlas_step_0(float4 rect_1, KernelContext_0 thread* kernelContext_3)
{
    return kernelContext_3->params_0->shadow_params_0.xy / rect_1.xy;
}


#line 628
float2 atlas_uv_0(float4 rect_2, float2 tile_uv_0)
{
    return rect_2.zw + tile_uv_0 * rect_2.xy;
}


#line 726
float tile_tap_0(float4 rect_3, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_4)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S2 = spoke_0.x;

#line 731
    float _S3 = rotation_0.x;

#line 731
    float _S4 = spoke_0.y;

#line 731
    float _S5 = rotation_0.y;


    float _S6 = ((kernelContext_4->shadow_atlas_0).sample_compare((kernelContext_4->shadow_sampler_0), (atlas_uv_0(rect_3, clamp(tile_uv_1 + float2(_S2 * _S3 - _S4 * _S5, _S2 * _S5 + _S4 * _S3) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 734
    return _S6;
}


#line 752
float tile_pcf_0(uint tile_1, float2 tile_uv_2, float reference_1, float2 pixel_2, float radius_0, KernelContext_0 thread* kernelContext_5)
{
    float2 _S7 = shadow_rotation_0(pixel_2);

#line 754
    float4 _S8 = atlas_rect_0(tile_1, kernelContext_5);

    if(atlas_rect_is_empty_0(_S8))
    {
        return 1.0f;
    }

#line 758
    float2 _S9 = atlas_step_0(_S8, kernelContext_5);

#line 758
    uint spot_0 = 0U;

#line 758
    float probe_0 = 0.0f;

#line 763
    for(;;)
    {

#line 763
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 763
            break;
        }

#line 763
        float _S10 = tile_tap_0(_S8, _S9, tile_uv_2, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_0) , _S7, reference_1, kernelContext_5);

        float probe_1 = probe_0 + _S10;

#line 763
        spot_0 = spot_0 + 1U;

#line 763
        probe_0 = probe_1;

#line 763
    }

#line 772
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 778
    uint index_1 = 0U;

#line 778
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 782
        if(index_1 < 32U)
        {
        }
        else
        {

#line 782
            break;
        }

#line 782
        float _S11 = tile_tap_0(_S8, _S9, tile_uv_2, SHADOW_DISC_0[index_1] * float2(radius_0) , _S7, reference_1, kernelContext_5);

        float visibility_1 = visibility_0 + _S11;

#line 782
        index_1 = index_1 + 1U;

#line 782
        visibility_0 = visibility_1;

#line 782
    }

#line 787
    return visibility_0 / 32.0f;
}


#line 806
float volumetric_sun_visibility_0(float3 world_position_0, float2 pixel_3, KernelContext_0 thread* kernelContext_6)
{

#line 806
    uint cascade_0;

#line 811
    float _S12 = length(world_position_0 - kernelContext_6->params_0->eye_0.xyz);

#line 811
    uint index_2 = 0U;

    for(;;)
    {

#line 813
        if(index_2 < 2U)
        {
        }
        else
        {

#line 813
            cascade_0 = 1U;

#line 813
            break;
        }
        if(_S12 < kernelContext_6->params_0->cascade_far_0[index_2])
        {

#line 815
            cascade_0 = index_2;


            break;
        }

#line 813
        index_2 = index_2 + 1U;

#line 813
    }

#line 822
    float4 clip_0 = (((float4(world_position_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(0)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(0)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(0)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(0)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(1)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(1)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(1)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(1)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(2)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(2)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(2)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(2)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(3)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(3)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(3)], (&kernelContext_6->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(3)]))));


    float3 ndc_1 = clip_0.xyz / float3(clip_0.w) ;

#line 825
    bool _S13;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 826
        _S13 = true;

#line 826
    }
    else
    {

#line 826
        _S13 = (ndc_1.z) <= 0.0f;

#line 826
    }

#line 826
    if(_S13)
    {
        return 1.0f;
    }

#line 828
    float _S14 = tile_pcf_0(cascade_0, float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_3, 2.0f, kernelContext_6);

#line 839
    return _S14;
}


#line 504
float range_window_0(float distance_0, float radius_1)
{
    float ratio_0 = distance_0 / max(radius_1, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}



float punctual_falloff_0(float distance_1, float radius_2)
{
    return range_window_0(distance_1, radius_2) / (distance_1 * distance_1 + 1.0f);
}


#line 523
float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 849
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S15 = axis_1.x;

#line 852
    float _S16 = axis_1.y;

#line 852
    bool _S17;

#line 852
    if(_S15 >= _S16)
    {

#line 852
        _S17 = _S15 >= (axis_1.z);

#line 852
    }
    else
    {

#line 852
        _S17 = false;

#line 852
    }

#line 852
    uint _S18;

#line 852
    if(_S17)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 854
            _S18 = 0U;

#line 854
        }
        else
        {

#line 854
            _S18 = 1U;

#line 854
        }

#line 854
        return _S18;
    }
    if(_S16 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 858
            _S18 = 2U;

#line 858
        }
        else
        {

#line 858
            _S18 = 3U;

#line 858
        }

#line 858
        return _S18;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 860
        _S18 = 4U;

#line 860
    }
    else
    {

#line 860
        _S18 = 5U;

#line 860
    }

#line 860
    return _S18;
}


#line 616
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 878
float volumetric_punctual_visibility_0(uint tile_3, float3 world_position_1, float2 pixel_4, KernelContext_0 thread* kernelContext_7)
{
    float4 clip_1 = (((float4(world_position_1, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(0)][int(0)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(1)][int(0)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(2)][int(0)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(3)][int(0)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(0)][int(1)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(1)][int(1)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(2)][int(1)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(3)][int(1)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(0)][int(2)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(1)][int(2)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(2)][int(2)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(3)][int(2)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(0)][int(3)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(1)][int(3)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(2)][int(3)], (&kernelContext_7->params_0->light_view_proj_0)->data_2[tile_3].data_0[int(3)][int(3)]))));
    float _S19 = clip_1.w;

#line 881
    if(_S19 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_2 = clip_1.xyz / float3(_S19) ;

#line 885
    bool _S20;
    if(any((abs(ndc_2.xy)) > (float2(1.0f) )))
    {

#line 886
        _S20 = true;

#line 886
    }
    else
    {

#line 886
        _S20 = (ndc_2.z) <= 0.0f;

#line 886
    }

#line 886
    if(_S20)
    {

#line 886
        _S20 = true;

#line 886
    }
    else
    {

#line 886
        _S20 = (ndc_2.z) > 1.0f;

#line 886
    }

#line 886
    if(_S20)
    {
        return 1.0f;
    }

#line 888
    float _S21 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_2.x * 0.5f + 0.5f, 0.5f - ndc_2.y * 0.5f), ndc_2.z, pixel_4, 2.0f, kernelContext_7);

#line 894
    return _S21;
}


#line 467
float volumetric_phase_0(float g_0, float cos_theta_0)
{
    float a_0 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    float _S22 = a_0 * a_0;

#line 470
    float d_0 = 1.0f + _S22 - 2.0f * a_0 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S22) / (d_0 * sqrt(d_0));
}


#line 555
float3 volumetric_punctual_0(uint froxel_0, float3 at_0, float3 view_direction_0, float2 pixel_5, KernelContext_0 thread* kernelContext_8)
{
    if((kernelContext_8->params_0->sun_radiance_0.w) <= 0.0f)
    {



        return float3(0.0f, 0.0f, 0.0f);
    }
    uint base_0 = froxel_0 * 17U;
    uint _S23 = min(kernelContext_8->cluster_lights_0[base_0], 16U);
    float3 _S24 = float3(0.0f, 0.0f, 0.0f);

#line 566
    uint slot_0 = 0U;

#line 566
    float3 total_0 = _S24;
    for(;;)
    {

#line 567
        if(slot_0 < _S23)
        {
        }
        else
        {

#line 567
            break;
        }
        GpuLight_natural_0 light_0 = kernelContext_8->lights_0[kernelContext_8->cluster_lights_0[base_0 + 1U + slot_0]];
        if((light_0.kind_0) == 0U)
        {
            slot_0 = slot_0 + 1U;

#line 567
            continue;
        }

#line 567
        float4 _S25 = float4(light_0.position_0) ;

#line 574
        float3 _S26 = _S25.xyz;

#line 574
        float3 offset_0 = _S26 - at_0;
        float distance_2 = length(offset_0);
        float3 to_light_1 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
        float reach_0 = punctual_falloff_0(distance_2, _S25.w);

#line 577
        float reach_1;
        if((light_0.kind_0) == 2U)
        {

#line 578
            float4 _S27 = float4(light_0.direction_0) ;

#line 578
            reach_1 = reach_0 * spot_cone_0(to_light_1, _S27.xyz, _S27.w, light_0.cos_inner_0);

#line 578
        }
        else
        {

#line 578
            reach_1 = reach_0;

#line 578
        }



        if(reach_1 <= 0.0f)
        {


            slot_0 = slot_0 + 1U;

#line 567
            continue;
        }

#line 567
        float reach_2;

#line 592
        if((light_0.kind_0) == 1U)
        {
            if((light_0.shadow_tile_0) <= 8U)
            {

#line 594
                float _S28 = volumetric_punctual_visibility_0(light_0.shadow_tile_0 + point_face_0(at_0 - _S26), at_0, pixel_5, kernelContext_8);

#line 594
                reach_2 = reach_1 * _S28;

#line 594
            }
            else
            {

#line 594
                reach_2 = reach_1;

#line 594
            }

#line 592
        }
        else
        {

#line 600
            if((light_0.shadow_tile_0) < 14U)
            {

#line 600
                float _S29 = volumetric_punctual_visibility_0(light_0.shadow_tile_0, at_0, pixel_5, kernelContext_8);

#line 600
                reach_2 = reach_1 * _S29;

#line 600
            }
            else
            {

#line 600
                reach_2 = reach_1;

#line 600
            }

#line 592
        }

#line 592
        total_0 = total_0 + (float4(light_0.color_0) ).xyz * float3(reach_2)  * float3(volumetric_phase_0(kernelContext_8->params_0->sun_direction_0.w, dot(to_light_1, view_direction_0))) ;

#line 567
        slot_0 = slot_0 + 1U;

#line 567
    }

#line 607
    return total_0 * float3(kernelContext_8->params_0->sun_radiance_0.w) ;
}


#line 343
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);

    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S30 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 350
    float kernel_0 = 0.0001984127011383f;

#line 350
    int term_0 = int(6);

    for(;;)
    {

#line 352
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 352
            break;
        }
        float _S31 = kernel_0 * _S30 + FOG_KERNEL_0[term_0];

#line 352
        int term_1 = term_0 - int(1);

#line 352
        kernel_0 = _S31;

#line 352
        term_0 = term_1;

#line 352
    }

#line 357
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}



float fog_one_minus_exp_over_0(float d_1)
{
    if((abs(d_1)) < 0.125f)
    {
        float _S32 = - d_1;

#line 366
        float series_0 = 0.00833333376795053f;

#line 366
        int term_2 = int(3);

        for(;;)
        {

#line 368
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 368
                break;
            }
            float _S33 = series_0 * _S32 + FOG_RATIO_KERNEL_0[term_2];

#line 368
            int term_3 = term_2 - int(1);

#line 368
            series_0 = _S33;

#line 368
            term_2 = term_3;

#line 368
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_1)) / d_1;
}



float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_3)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_3, 0.0f, 32.0f);
    }

#line 390
    return clamp(density_0 * distance_3 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 490
float3 volumetric_source_0(float3 view_direction_1, float4 lit_0, KernelContext_0 thread* kernelContext_9)
{



    return kernelContext_9->params_0->fog_color_0.xyz + kernelContext_9->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_9->params_0->sun_direction_0.w, dot(kernelContext_9->params_0->sun_direction_0.xyz, view_direction_1)))  * float3(lit_0.w)  + lit_0.xyz;
}


#line 908
float4 volumetric_slice_0(float3 from_0, float3 to_0, float3 view_direction_2, float4 lit_1, KernelContext_0 thread* kernelContext_10)
{
    float reference_2 = kernelContext_10->params_0->fog_params_0.z;



    float survives_0 = fog_exp_neg_0(fog_optical_depth_0(kernelContext_10->params_0->fog_params_0.x, kernelContext_10->params_0->fog_params_0.y, from_0.y - reference_2, to_0.y - reference_2, length(to_0 - from_0)));

#line 914
    float3 _S34 = volumetric_source_0(view_direction_2, lit_1, kernelContext_10);
    return float4(_S34 * float3((1.0f - survives_0)) , survives_0);
}


#line 925
[[kernel]] void scatterMain(uint3 thread_0 [[thread_position_in_grid]], VolumetricParams_natural_0 constant* params_1 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(0)]], sampler shadow_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(4)]], GpuLight_natural_0 device* lights_1 [[buffer(3)]], packed_float4 device* lighting_1 [[buffer(2)]], packed_float4 device* volumetrics_1 [[buffer(1)]])
{

#line 925
    thread KernelContext_0 kernelContext_11;

#line 925
    (&kernelContext_11)->params_0 = params_1;

#line 925
    (&kernelContext_11)->shadow_atlas_0 = shadow_atlas_1;

#line 925
    (&kernelContext_11)->shadow_sampler_0 = shadow_sampler_1;

#line 925
    (&kernelContext_11)->cluster_lights_0 = cluster_lights_1;

#line 925
    (&kernelContext_11)->lights_0 = lights_1;

#line 925
    (&kernelContext_11)->lighting_0 = lighting_1;

#line 925
    (&kernelContext_11)->volumetrics_0 = volumetrics_1;

    uint froxel_1 = thread_0.x;
    uint tiles_0 = max(params_1->grid_x_0, 1U) * max(params_1->grid_y_0, 1U);
    uint _S35 = max(params_1->slices_0, 1U);

#line 929
    bool _S36;
    if(froxel_1 >= (tiles_0 * _S35))
    {

#line 930
        _S36 = true;

#line 930
    }
    else
    {

#line 930
        _S36 = froxel_1 >= ((&kernelContext_11)->params_0->froxel_count_0);

#line 930
    }

#line 930
    if(_S36)
    {
        return;
    }

    uint tile_x_1 = froxel_1 % max(params_1->grid_x_0, 1U);
    uint _S37 = froxel_1 / max(params_1->grid_x_0, 1U);

#line 936
    uint tile_y_1 = _S37 % max(params_1->grid_y_0, 1U);
    uint slice_0 = froxel_1 / tiles_0;

    thread float3 near_point_1;
    thread float near_depth_1;

#line 940
    volumetric_tile_ray_0(tile_x_1, tile_y_1, &near_point_1, &near_depth_1, &kernelContext_11);

    float3 along_0 = (near_point_1 - (&kernelContext_11)->params_0->eye_0.xyz) / float3(near_depth_1) ;

#line 942
    float from_depth_0;

#line 952
    if(slice_0 == 0U)
    {

#line 952
        from_depth_0 = 0.0f;

#line 952
    }
    else
    {

#line 952
        from_depth_0 = volumetric_slice_start_0(slice_0);

#line 952
    }
    uint _S38 = slice_0 + 1U;

#line 953
    float to_depth_0;

#line 953
    if(_S38 == _S35)
    {

#line 953
        to_depth_0 = 1000.0f;

#line 953
    }
    else
    {

#line 953
        to_depth_0 = volumetric_slice_start_0(_S38);

#line 953
    }

    float3 from_1 = (&kernelContext_11)->params_0->eye_0.xyz + along_0 * float3(from_depth_0) ;
    float3 to_1 = (&kernelContext_11)->params_0->eye_0.xyz + along_0 * float3(to_depth_0) ;

#line 969
    float3 middle_0 = (from_1 + to_1) * float3(0.5f) ;
    float2 pixel_6 = float2(float(tile_x_1), float(tile_y_1));

#line 970
    float _S39 = volumetric_sun_visibility_0(middle_0, pixel_6, &kernelContext_11);

#line 975
    float3 segment_0 = to_1 - from_1;
    float length_of_0 = length(segment_0);

#line 976
    float3 view_direction_3;
    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 977
        view_direction_3 = segment_0 / float3(length_of_0) ;

#line 977
    }
    else
    {

#line 977
        view_direction_3 = float3(0.0f, 0.0f, 1.0f);

#line 977
    }

#line 977
    float3 _S40 = volumetric_punctual_0(froxel_1, middle_0, view_direction_3, pixel_6, &kernelContext_11);
    float4 lit_2 = float4(_S40, _S39);

#line 978
    *((&kernelContext_11)->lighting_0+froxel_1) = packed_float4(lit_2) ;

#line 978
    packed_float4 device* _S41 = (&kernelContext_11)->volumetrics_0+froxel_1;

#line 978
    float4 _S42 = volumetric_slice_0(from_1, to_1, view_direction_3, lit_2, &kernelContext_11);

#line 978
    *_S41 = packed_float4(_S42) ;


    return;
}


#line 993
[[kernel]] void integrateMain(uint3 thread_1 [[thread_position_in_grid]], VolumetricParams_natural_0 constant* params_2 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(0)]], sampler shadow_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(4)]], GpuLight_natural_0 device* lights_2 [[buffer(3)]], packed_float4 device* lighting_2 [[buffer(2)]], packed_float4 device* volumetrics_2 [[buffer(1)]])
{

#line 993
    thread KernelContext_0 kernelContext_12;

#line 993
    (&kernelContext_12)->params_0 = params_2;

#line 993
    (&kernelContext_12)->shadow_atlas_0 = shadow_atlas_2;

#line 993
    (&kernelContext_12)->shadow_sampler_0 = shadow_sampler_2;

#line 993
    (&kernelContext_12)->cluster_lights_0 = cluster_lights_2;

#line 993
    (&kernelContext_12)->lights_0 = lights_2;

#line 993
    (&kernelContext_12)->lighting_0 = lighting_2;

#line 993
    (&kernelContext_12)->volumetrics_0 = volumetrics_2;

    uint tile_4 = thread_1.x;
    uint tiles_1 = max(params_2->grid_x_0, 1U) * max(params_2->grid_y_0, 1U);
    if(tile_4 >= tiles_1)
    {
        return;
    }
    uint _S43 = max((&kernelContext_12)->params_0->slices_0, 1U);

    float3 _S44 = float3(0.0f, 0.0f, 0.0f);

#line 1003
    uint slice_1 = 0U;

#line 1003
    float3 accumulated_0 = _S44;

#line 1003
    float through_0 = 1.0f;

    for(;;)
    {

#line 1005
        if(slice_1 < _S43)
        {
        }
        else
        {

#line 1005
            break;
        }
        uint froxel_2 = tile_4 + slice_1 * tiles_1;
        if(froxel_2 >= ((&kernelContext_12)->params_0->froxel_count_0))
        {
            break;
        }

#line 1010
        float4 _S45 = float4(*((&kernelContext_12)->volumetrics_0+froxel_2)) ;

#line 1010
        *((&kernelContext_12)->volumetrics_0+froxel_2) = packed_float4(float4(accumulated_0, through_0)) ;



        float3 accumulated_1 = accumulated_0 + float3(through_0)  * _S45.xyz;
        float through_1 = through_0 * _S45.w;

#line 1005
        slice_1 = slice_1 + 1U;

#line 1005
        accumulated_0 = accumulated_1;

#line 1005
        through_0 = through_1;

#line 1005
    }

#line 1017
    return;
}

