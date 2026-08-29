#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 99 "shaders/volumetric.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 94
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 596
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 623
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 636
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 90
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 90
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(14)> data_2;
};


#line 90
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
};


#line 516 "shaders/volumetric.slang"
struct GpuLight_natural_0
{
    packed_float4 position_0;
    packed_float4 color_0;
    packed_float4 direction_0;
    uint kind_0;
    float cos_inner_0;
    uint shadow_tile_0;
    uint pad1_0;
};


#line 516
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


#line 352
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 388
void volumetric_tile_ray_0(uint tile_x_0, uint tile_y_0, float3 thread* near_point_0, float thread* near_depth_0, KernelContext_0 thread* kernelContext_1)
{

    float2 pixel_0 = (float2(float(tile_x_0), float(tile_y_0)) + float2(0.5f) ) * float2(float(kernelContext_1->params_0->tile_pixels_0)) ;

#line 391
    float3 _S1 = volumetric_unproject_0(float2(pixel_0.x / float(max(kernelContext_1->params_0->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - pixel_0.y / float(max(kernelContext_1->params_0->viewport_y_0, 1U)) * 2.0f), 1.0f, kernelContext_1);



    *near_point_0 = _S1;
    *near_depth_0 = max(dot(kernelContext_1->params_0->depth_row_0, float4(_S1, 1.0f)), 9.99999997475242708e-07f);
    return;
}


#line 367
float volumetric_slice_start_0(uint index_0)
{

#line 367
    uint step_0 = 0U;

#line 367
    float start_0 = 0.10000000149011612f;


    for(;;)
    {

#line 370
        if(step_0 < index_0)
        {
        }
        else
        {

#line 370
            break;
        }
        float start_1 = start_0 * 1.46779930591583252f;

#line 370
        step_0 = step_0 + 1U;

#line 370
        start_0 = start_1;

#line 370
    }



    return start_0;
}


#line 645
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 579
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 4.0f);
}


#line 654
float tile_tap_0(uint tile_1, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_2)
{
    float2 texel_0 = kernelContext_2->params_0->shadow_params_0.xy;

#line 661
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_0 * grid_0;

    float _S2 = spoke_0.x;

#line 664
    float _S3 = rotation_0.x;

#line 664
    float _S4 = spoke_0.y;

#line 664
    float _S5 = rotation_0.y;


    float _S6 = ((kernelContext_2->shadow_atlas_0).sample_compare((kernelContext_2->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(_S2 * _S3 - _S4 * _S5, _S2 * _S5 + _S4 * _S3) * texel_0 * grid_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 667
    return _S6;
}


#line 685
float tile_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, float2 pixel_2, float radius_0, KernelContext_0 thread* kernelContext_3)
{
    float2 _S7 = shadow_rotation_0(pixel_2);

#line 687
    uint spot_0 = 0U;

#line 687
    float probe_0 = 0.0f;


    for(;;)
    {

#line 690
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 690
            break;
        }

#line 690
        float _S8 = tile_tap_0(tile_2, tile_uv_2, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_0) , _S7, reference_1, kernelContext_3);

        float probe_1 = probe_0 + _S8;

#line 690
        spot_0 = spot_0 + 1U;

#line 690
        probe_0 = probe_1;

#line 690
    }

#line 699
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 705
    uint index_1 = 0U;

#line 705
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 709
        if(index_1 < 32U)
        {
        }
        else
        {

#line 709
            break;
        }

#line 709
        float _S9 = tile_tap_0(tile_2, tile_uv_2, SHADOW_DISC_0[index_1] * float2(radius_0) , _S7, reference_1, kernelContext_3);

        float visibility_1 = visibility_0 + _S9;

#line 709
        index_1 = index_1 + 1U;

#line 709
        visibility_0 = visibility_1;

#line 709
    }



    return visibility_0 / 32.0f;
}


#line 732
float volumetric_sun_visibility_0(float3 world_position_0, float2 pixel_3, KernelContext_0 thread* kernelContext_4)
{

#line 732
    uint cascade_0;

#line 737
    float _S10 = length(world_position_0 - kernelContext_4->params_0->eye_0.xyz);

#line 737
    uint index_2 = 0U;

    for(;;)
    {

#line 739
        if(index_2 < 2U)
        {
        }
        else
        {

#line 739
            cascade_0 = 1U;

#line 739
            break;
        }
        if(_S10 < kernelContext_4->params_0->cascade_far_0[index_2])
        {

#line 741
            cascade_0 = index_2;


            break;
        }

#line 739
        index_2 = index_2 + 1U;

#line 739
    }

#line 748
    float4 clip_0 = (((float4(world_position_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(0)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(0)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(0)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(0)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(1)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(1)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(1)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(1)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(2)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(2)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(2)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(2)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(3)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(3)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(3)], (&kernelContext_4->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(3)]))));


    float3 ndc_1 = clip_0.xyz / float3(clip_0.w) ;

#line 751
    bool _S11;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 752
        _S11 = true;

#line 752
    }
    else
    {

#line 752
        _S11 = (ndc_1.z) <= 0.0f;

#line 752
    }

#line 752
    if(_S11)
    {
        return 1.0f;
    }

#line 754
    float _S12 = tile_pcf_0(cascade_0, float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_3, 2.0f, kernelContext_4);

#line 765
    return _S12;
}


#line 462
float punctual_falloff_0(float distance_0, float radius_1)
{
    float ratio_0 = distance_0 / max(radius_1, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 474
float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 775
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S13 = axis_1.x;

#line 778
    float _S14 = axis_1.y;

#line 778
    bool _S15;

#line 778
    if(_S13 >= _S14)
    {

#line 778
        _S15 = _S13 >= (axis_1.z);

#line 778
    }
    else
    {

#line 778
        _S15 = false;

#line 778
    }

#line 778
    uint _S16;

#line 778
    if(_S15)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 780
            _S16 = 0U;

#line 780
        }
        else
        {

#line 780
            _S16 = 1U;

#line 780
        }

#line 780
        return _S16;
    }
    if(_S14 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 784
            _S16 = 2U;

#line 784
        }
        else
        {

#line 784
            _S16 = 3U;

#line 784
        }

#line 784
        return _S16;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 786
        _S16 = 4U;

#line 786
    }
    else
    {

#line 786
        _S16 = 5U;

#line 786
    }

#line 786
    return _S16;
}


#line 567
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 804
float volumetric_punctual_visibility_0(uint tile_4, float3 world_position_1, float2 pixel_4, KernelContext_0 thread* kernelContext_5)
{
    float4 clip_1 = (((float4(world_position_1, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(0)][int(0)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(1)][int(0)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(2)][int(0)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(3)][int(0)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(0)][int(1)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(1)][int(1)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(2)][int(1)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(3)][int(1)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(0)][int(2)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(1)][int(2)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(2)][int(2)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(3)][int(2)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(0)][int(3)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(1)][int(3)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(2)][int(3)], (&kernelContext_5->params_0->light_view_proj_0)->data_2[tile_4].data_0[int(3)][int(3)]))));
    float _S17 = clip_1.w;

#line 807
    if(_S17 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_2 = clip_1.xyz / float3(_S17) ;

#line 811
    bool _S18;
    if(any((abs(ndc_2.xy)) > (float2(1.0f) )))
    {

#line 812
        _S18 = true;

#line 812
    }
    else
    {

#line 812
        _S18 = (ndc_2.z) <= 0.0f;

#line 812
    }

#line 812
    if(_S18)
    {

#line 812
        _S18 = true;

#line 812
    }
    else
    {

#line 812
        _S18 = (ndc_2.z) > 1.0f;

#line 812
    }

#line 812
    if(_S18)
    {
        return 1.0f;
    }

#line 814
    float _S19 = tile_pcf_0(light_tile_0(tile_4), float2(ndc_2.x * 0.5f + 0.5f, 0.5f - ndc_2.y * 0.5f), ndc_2.z, pixel_4, 2.0f, kernelContext_5);

#line 820
    return _S19;
}


#line 425
float volumetric_phase_0(float g_0, float cos_theta_0)
{
    float a_0 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    float _S20 = a_0 * a_0;

#line 428
    float d_0 = 1.0f + _S20 - 2.0f * a_0 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S20) / (d_0 * sqrt(d_0));
}


#line 506
float3 volumetric_punctual_0(uint froxel_0, float3 at_0, float3 view_direction_0, float2 pixel_5, KernelContext_0 thread* kernelContext_6)
{
    if((kernelContext_6->params_0->sun_radiance_0.w) <= 0.0f)
    {



        return float3(0.0f, 0.0f, 0.0f);
    }
    uint base_0 = froxel_0 * 17U;
    uint _S21 = min(kernelContext_6->cluster_lights_0[base_0], 16U);
    float3 _S22 = float3(0.0f, 0.0f, 0.0f);

#line 517
    uint slot_0 = 0U;

#line 517
    float3 total_0 = _S22;
    for(;;)
    {

#line 518
        if(slot_0 < _S21)
        {
        }
        else
        {

#line 518
            break;
        }
        GpuLight_natural_0 light_0 = kernelContext_6->lights_0[kernelContext_6->cluster_lights_0[base_0 + 1U + slot_0]];
        if((light_0.kind_0) == 0U)
        {
            slot_0 = slot_0 + 1U;

#line 518
            continue;
        }

#line 518
        float4 _S23 = float4(light_0.position_0) ;

#line 525
        float3 _S24 = _S23.xyz;

#line 525
        float3 offset_0 = _S24 - at_0;
        float distance_1 = length(offset_0);
        float3 to_light_1 = offset_0 / float3(max(distance_1, 9.99999997475242708e-07f)) ;
        float reach_0 = punctual_falloff_0(distance_1, _S23.w);

#line 528
        float reach_1;
        if((light_0.kind_0) == 2U)
        {

#line 529
            float4 _S25 = float4(light_0.direction_0) ;

#line 529
            reach_1 = reach_0 * spot_cone_0(to_light_1, _S25.xyz, _S25.w, light_0.cos_inner_0);

#line 529
        }
        else
        {

#line 529
            reach_1 = reach_0;

#line 529
        }



        if(reach_1 <= 0.0f)
        {


            slot_0 = slot_0 + 1U;

#line 518
            continue;
        }

#line 518
        float reach_2;

#line 543
        if((light_0.kind_0) == 1U)
        {
            if((light_0.shadow_tile_0) <= 8U)
            {

#line 545
                float _S26 = volumetric_punctual_visibility_0(light_0.shadow_tile_0 + point_face_0(at_0 - _S24), at_0, pixel_5, kernelContext_6);

#line 545
                reach_2 = reach_1 * _S26;

#line 545
            }
            else
            {

#line 545
                reach_2 = reach_1;

#line 545
            }

#line 543
        }
        else
        {

#line 551
            if((light_0.shadow_tile_0) < 14U)
            {

#line 551
                float _S27 = volumetric_punctual_visibility_0(light_0.shadow_tile_0, at_0, pixel_5, kernelContext_6);

#line 551
                reach_2 = reach_1 * _S27;

#line 551
            }
            else
            {

#line 551
                reach_2 = reach_1;

#line 551
            }

#line 543
        }

#line 543
        total_0 = total_0 + (float4(light_0.color_0) ).xyz * float3(reach_2)  * float3(volumetric_phase_0(kernelContext_6->params_0->sun_direction_0.w, dot(to_light_1, view_direction_0))) ;

#line 518
        slot_0 = slot_0 + 1U;

#line 518
    }

#line 558
    return total_0 * float3(kernelContext_6->params_0->sun_radiance_0.w) ;
}


#line 301
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);

    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S28 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 308
    float kernel_0 = 0.0001984127011383f;

#line 308
    int term_0 = int(6);

    for(;;)
    {

#line 310
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 310
            break;
        }
        float _S29 = kernel_0 * _S28 + FOG_KERNEL_0[term_0];

#line 310
        int term_1 = term_0 - int(1);

#line 310
        kernel_0 = _S29;

#line 310
        term_0 = term_1;

#line 310
    }

#line 315
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}



float fog_one_minus_exp_over_0(float d_1)
{
    if((abs(d_1)) < 0.125f)
    {
        float _S30 = - d_1;

#line 324
        float series_0 = 0.00833333376795053f;

#line 324
        int term_2 = int(3);

        for(;;)
        {

#line 326
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 326
                break;
            }
            float _S31 = series_0 * _S30 + FOG_RATIO_KERNEL_0[term_2];

#line 326
            int term_3 = term_2 - int(1);

#line 326
            series_0 = _S31;

#line 326
            term_2 = term_3;

#line 326
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_1)) / d_1;
}



float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 348
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 448
float3 volumetric_source_0(float3 view_direction_1, float4 lit_0, KernelContext_0 thread* kernelContext_7)
{



    return kernelContext_7->params_0->fog_color_0.xyz + kernelContext_7->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_7->params_0->sun_direction_0.w, dot(kernelContext_7->params_0->sun_direction_0.xyz, view_direction_1)))  * float3(lit_0.w)  + lit_0.xyz;
}


#line 834
float4 volumetric_slice_0(float3 from_0, float3 to_0, float3 view_direction_2, float4 lit_1, KernelContext_0 thread* kernelContext_8)
{
    float reference_2 = kernelContext_8->params_0->fog_params_0.z;



    float survives_0 = fog_exp_neg_0(fog_optical_depth_0(kernelContext_8->params_0->fog_params_0.x, kernelContext_8->params_0->fog_params_0.y, from_0.y - reference_2, to_0.y - reference_2, length(to_0 - from_0)));

#line 840
    float3 _S32 = volumetric_source_0(view_direction_2, lit_1, kernelContext_8);
    return float4(_S32 * float3((1.0f - survives_0)) , survives_0);
}


#line 851
[[kernel]] void scatterMain(uint3 thread_0 [[thread_position_in_grid]], VolumetricParams_natural_0 constant* params_1 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(0)]], sampler shadow_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(4)]], GpuLight_natural_0 device* lights_1 [[buffer(3)]], packed_float4 device* lighting_1 [[buffer(2)]], packed_float4 device* volumetrics_1 [[buffer(1)]])
{

#line 851
    thread KernelContext_0 kernelContext_9;

#line 851
    (&kernelContext_9)->params_0 = params_1;

#line 851
    (&kernelContext_9)->shadow_atlas_0 = shadow_atlas_1;

#line 851
    (&kernelContext_9)->shadow_sampler_0 = shadow_sampler_1;

#line 851
    (&kernelContext_9)->cluster_lights_0 = cluster_lights_1;

#line 851
    (&kernelContext_9)->lights_0 = lights_1;

#line 851
    (&kernelContext_9)->lighting_0 = lighting_1;

#line 851
    (&kernelContext_9)->volumetrics_0 = volumetrics_1;

    uint froxel_1 = thread_0.x;
    uint tiles_0 = max(params_1->grid_x_0, 1U) * max(params_1->grid_y_0, 1U);
    uint _S33 = max(params_1->slices_0, 1U);

#line 855
    bool _S34;
    if(froxel_1 >= (tiles_0 * _S33))
    {

#line 856
        _S34 = true;

#line 856
    }
    else
    {

#line 856
        _S34 = froxel_1 >= ((&kernelContext_9)->params_0->froxel_count_0);

#line 856
    }

#line 856
    if(_S34)
    {
        return;
    }

    uint tile_x_1 = froxel_1 % max(params_1->grid_x_0, 1U);
    uint _S35 = froxel_1 / max(params_1->grid_x_0, 1U);

#line 862
    uint tile_y_1 = _S35 % max(params_1->grid_y_0, 1U);
    uint slice_0 = froxel_1 / tiles_0;

    thread float3 near_point_1;
    thread float near_depth_1;

#line 866
    volumetric_tile_ray_0(tile_x_1, tile_y_1, &near_point_1, &near_depth_1, &kernelContext_9);

    float3 along_0 = (near_point_1 - (&kernelContext_9)->params_0->eye_0.xyz) / float3(near_depth_1) ;

#line 868
    float from_depth_0;

#line 878
    if(slice_0 == 0U)
    {

#line 878
        from_depth_0 = 0.0f;

#line 878
    }
    else
    {

#line 878
        from_depth_0 = volumetric_slice_start_0(slice_0);

#line 878
    }
    uint _S36 = slice_0 + 1U;

#line 879
    float to_depth_0;

#line 879
    if(_S36 == _S33)
    {

#line 879
        to_depth_0 = 1000.0f;

#line 879
    }
    else
    {

#line 879
        to_depth_0 = volumetric_slice_start_0(_S36);

#line 879
    }

    float3 from_1 = (&kernelContext_9)->params_0->eye_0.xyz + along_0 * float3(from_depth_0) ;
    float3 to_1 = (&kernelContext_9)->params_0->eye_0.xyz + along_0 * float3(to_depth_0) ;

#line 895
    float3 middle_0 = (from_1 + to_1) * float3(0.5f) ;
    float2 pixel_6 = float2(float(tile_x_1), float(tile_y_1));

#line 896
    float _S37 = volumetric_sun_visibility_0(middle_0, pixel_6, &kernelContext_9);

#line 901
    float3 segment_0 = to_1 - from_1;
    float length_of_0 = length(segment_0);

#line 902
    float3 view_direction_3;
    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 903
        view_direction_3 = segment_0 / float3(length_of_0) ;

#line 903
    }
    else
    {

#line 903
        view_direction_3 = float3(0.0f, 0.0f, 1.0f);

#line 903
    }

#line 903
    float3 _S38 = volumetric_punctual_0(froxel_1, middle_0, view_direction_3, pixel_6, &kernelContext_9);
    float4 lit_2 = float4(_S38, _S37);

#line 904
    *((&kernelContext_9)->lighting_0+froxel_1) = packed_float4(lit_2) ;

#line 904
    packed_float4 device* _S39 = (&kernelContext_9)->volumetrics_0+froxel_1;

#line 904
    float4 _S40 = volumetric_slice_0(from_1, to_1, view_direction_3, lit_2, &kernelContext_9);

#line 904
    *_S39 = packed_float4(_S40) ;


    return;
}


#line 919
[[kernel]] void integrateMain(uint3 thread_1 [[thread_position_in_grid]], VolumetricParams_natural_0 constant* params_2 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(0)]], sampler shadow_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(4)]], GpuLight_natural_0 device* lights_2 [[buffer(3)]], packed_float4 device* lighting_2 [[buffer(2)]], packed_float4 device* volumetrics_2 [[buffer(1)]])
{

#line 919
    thread KernelContext_0 kernelContext_10;

#line 919
    (&kernelContext_10)->params_0 = params_2;

#line 919
    (&kernelContext_10)->shadow_atlas_0 = shadow_atlas_2;

#line 919
    (&kernelContext_10)->shadow_sampler_0 = shadow_sampler_2;

#line 919
    (&kernelContext_10)->cluster_lights_0 = cluster_lights_2;

#line 919
    (&kernelContext_10)->lights_0 = lights_2;

#line 919
    (&kernelContext_10)->lighting_0 = lighting_2;

#line 919
    (&kernelContext_10)->volumetrics_0 = volumetrics_2;

    uint tile_5 = thread_1.x;
    uint tiles_1 = max(params_2->grid_x_0, 1U) * max(params_2->grid_y_0, 1U);
    if(tile_5 >= tiles_1)
    {
        return;
    }
    uint _S41 = max((&kernelContext_10)->params_0->slices_0, 1U);

    float3 _S42 = float3(0.0f, 0.0f, 0.0f);

#line 929
    uint slice_1 = 0U;

#line 929
    float3 accumulated_0 = _S42;

#line 929
    float through_0 = 1.0f;

    for(;;)
    {

#line 931
        if(slice_1 < _S41)
        {
        }
        else
        {

#line 931
            break;
        }
        uint froxel_2 = tile_5 + slice_1 * tiles_1;
        if(froxel_2 >= ((&kernelContext_10)->params_0->froxel_count_0))
        {
            break;
        }

#line 936
        float4 _S43 = float4(*((&kernelContext_10)->volumetrics_0+froxel_2)) ;

#line 936
        *((&kernelContext_10)->volumetrics_0+froxel_2) = packed_float4(float4(accumulated_0, through_0)) ;



        float3 accumulated_1 = accumulated_0 + float3(through_0)  * _S43.xyz;
        float through_1 = through_0 * _S43.w;

#line 931
        slice_1 = slice_1 + 1U;

#line 931
        accumulated_0 = accumulated_1;

#line 931
        through_0 = through_1;

#line 931
    }

#line 943
    return;
}

