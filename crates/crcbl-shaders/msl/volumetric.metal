#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 99 "shaders/volumetric.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 94
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 408
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 427
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 438
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
};


#line 627 "shaders/volumetric.slang"
struct KernelContext_0
{
    VolumetricParams_natural_0 constant* params_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    float device* visibilities_0;
    packed_float4 device* volumetrics_0;
};


#line 282
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 318
void volumetric_tile_ray_0(uint tile_x_0, uint tile_y_0, float3 thread* near_point_0, float thread* near_depth_0, KernelContext_0 thread* kernelContext_1)
{

    float2 pixel_0 = (float2(float(tile_x_0), float(tile_y_0)) + float2(0.5f) ) * float2(float(kernelContext_1->params_0->tile_pixels_0)) ;

#line 321
    float3 _S1 = volumetric_unproject_0(float2(pixel_0.x / float(max(kernelContext_1->params_0->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - pixel_0.y / float(max(kernelContext_1->params_0->viewport_y_0, 1U)) * 2.0f), 1.0f, kernelContext_1);



    *near_point_0 = _S1;
    *near_depth_0 = max(dot(kernelContext_1->params_0->depth_row_0, float4(_S1, 1.0f)), 9.99999997475242708e-07f);
    return;
}


#line 297
float volumetric_slice_start_0(uint index_0)
{

#line 297
    uint step_0 = 0U;

#line 297
    float start_0 = 0.10000000149011612f;


    for(;;)
    {

#line 300
        if(step_0 < index_0)
        {
        }
        else
        {

#line 300
            break;
        }
        float start_1 = start_0 * 1.46779930591583252f;

#line 300
        step_0 = step_0 + 1U;

#line 300
        start_0 = start_1;

#line 300
    }



    return start_0;
}


#line 447
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 391
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 4.0f);
}


#line 470
float tile_pcf_0(uint tile_1, float2 tile_uv_1, float reference_0, float2 pixel_2, float radius_0, KernelContext_0 thread* kernelContext_2)
{
    float2 texel_0 = kernelContext_2->params_0->shadow_params_0.xy;

#line 477
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S2 = float2(0.5f, 0.5f) * texel_0 * grid_0;

    float2 _S3 = shadow_rotation_0(pixel_2);

#line 480
    uint index_1 = 0U;

#line 480
    float visibility_0 = 0.0f;


    for(;;)
    {

#line 483
        if(index_1 < 32U)
        {
        }
        else
        {

#line 483
            break;
        }
        float2 spoke_0 = SHADOW_DISC_0[index_1] * float2(radius_0) ;

        float _S4 = spoke_0.x;

#line 487
        float _S5 = _S3.x;

#line 487
        float _S6 = spoke_0.y;

#line 487
        float _S7 = _S3.y;



        float _S8 = ((kernelContext_2->shadow_atlas_0).sample_compare((kernelContext_2->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(_S4 * _S5 - _S6 * _S7, _S4 * _S7 + _S6 * _S5) * texel_0 * grid_0, _S2, float2(1.0f)  - _S2))), (reference_0), level((0.0f))));

#line 490
        float visibility_1 = visibility_0 + _S8;

#line 483
        index_1 = index_1 + 1U;

#line 483
        visibility_0 = visibility_1;

#line 483
    }

#line 493
    return visibility_0 / 32.0f;
}


#line 512
float volumetric_sun_visibility_0(float3 world_position_0, float2 pixel_3, KernelContext_0 thread* kernelContext_3)
{

#line 512
    uint cascade_0;

#line 517
    float _S9 = length(world_position_0 - kernelContext_3->params_0->eye_0.xyz);

#line 517
    uint index_2 = 0U;

    for(;;)
    {

#line 519
        if(index_2 < 2U)
        {
        }
        else
        {

#line 519
            cascade_0 = 1U;

#line 519
            break;
        }
        if(_S9 < kernelContext_3->params_0->cascade_far_0[index_2])
        {

#line 521
            cascade_0 = index_2;


            break;
        }

#line 519
        index_2 = index_2 + 1U;

#line 519
    }

#line 528
    float4 clip_0 = (((float4(world_position_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(0)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(0)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(0)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(0)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(1)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(1)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(1)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(1)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(2)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(2)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(2)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(2)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(0)][int(3)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(1)][int(3)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(2)][int(3)], (&kernelContext_3->params_0->shadow_view_proj_0)->data_1[cascade_0].data_0[int(3)][int(3)]))));


    float3 ndc_1 = clip_0.xyz / float3(clip_0.w) ;

#line 531
    bool _S10;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 532
        _S10 = true;

#line 532
    }
    else
    {

#line 532
        _S10 = (ndc_1.z) <= 0.0f;

#line 532
    }

#line 532
    if(_S10)
    {
        return 1.0f;
    }

#line 534
    float _S11 = tile_pcf_0(cascade_0, float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_3, 2.0f, kernelContext_3);

#line 545
    return _S11;
}


#line 231
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);

    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S12 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 238
    float kernel_0 = 0.0001984127011383f;

#line 238
    int term_0 = int(6);

    for(;;)
    {

#line 240
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 240
            break;
        }
        float _S13 = kernel_0 * _S12 + FOG_KERNEL_0[term_0];

#line 240
        int term_1 = term_0 - int(1);

#line 240
        kernel_0 = _S13;

#line 240
        term_0 = term_1;

#line 240
    }

#line 245
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}



float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S14 = - d_0;

#line 254
        float series_0 = 0.00833333376795053f;

#line 254
        int term_2 = int(3);

        for(;;)
        {

#line 256
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 256
                break;
            }
            float _S15 = series_0 * _S14 + FOG_RATIO_KERNEL_0[term_2];

#line 256
            int term_3 = term_2 - int(1);

#line 256
            series_0 = _S15;

#line 256
            term_2 = term_3;

#line 256
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}



float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_0)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_0, 0.0f, 32.0f);
    }

#line 278
    return clamp(density_0 * distance_0 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 355
float volumetric_phase_0(float g_0, float cos_theta_0)
{
    float a_0 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    float _S16 = a_0 * a_0;

#line 358
    float d_1 = 1.0f + _S16 - 2.0f * a_0 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S16) / (d_1 * sqrt(d_1));
}


#line 376
float3 volumetric_source_0(float3 view_direction_0, float visibility_2, KernelContext_0 thread* kernelContext_4)
{



    return kernelContext_4->params_0->fog_color_0.xyz + kernelContext_4->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_4->params_0->sun_direction_0.w, dot(kernelContext_4->params_0->sun_direction_0.xyz, view_direction_0)))  * float3(visibility_2) ;
}


#line 559
float4 volumetric_slice_0(float3 from_0, float3 to_0, float visibility_3, KernelContext_0 thread* kernelContext_5)
{
    float reference_1 = kernelContext_5->params_0->fog_params_0.z;
    float3 segment_0 = to_0 - from_0;
    float length_of_0 = length(segment_0);


    float survives_0 = fog_exp_neg_0(fog_optical_depth_0(kernelContext_5->params_0->fog_params_0.x, kernelContext_5->params_0->fog_params_0.y, from_0.y - reference_1, to_0.y - reference_1, length_of_0));

#line 566
    float3 view_direction_1;



    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 570
        view_direction_1 = segment_0 / float3(length_of_0) ;

#line 570
    }
    else
    {

#line 570
        view_direction_1 = float3(0.0f, 0.0f, 1.0f);

#line 570
    }

#line 570
    float3 _S17 = volumetric_source_0(view_direction_1, visibility_3, kernelContext_5);
    return float4(_S17 * float3((1.0f - survives_0)) , survives_0);
}


#line 581
[[kernel]] void scatterMain(uint3 thread_0 [[thread_position_in_grid]], VolumetricParams_natural_0 constant* params_1 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(0)]], sampler shadow_sampler_1 [[sampler(0)]], float device* visibilities_1 [[buffer(2)]], packed_float4 device* volumetrics_1 [[buffer(1)]])
{

#line 581
    thread KernelContext_0 kernelContext_6;

#line 581
    (&kernelContext_6)->params_0 = params_1;

#line 581
    (&kernelContext_6)->shadow_atlas_0 = shadow_atlas_1;

#line 581
    (&kernelContext_6)->shadow_sampler_0 = shadow_sampler_1;

#line 581
    (&kernelContext_6)->visibilities_0 = visibilities_1;

#line 581
    (&kernelContext_6)->volumetrics_0 = volumetrics_1;

    uint froxel_0 = thread_0.x;
    uint tiles_0 = max(params_1->grid_x_0, 1U) * max(params_1->grid_y_0, 1U);
    uint _S18 = max(params_1->slices_0, 1U);

#line 585
    bool _S19;
    if(froxel_0 >= (tiles_0 * _S18))
    {

#line 586
        _S19 = true;

#line 586
    }
    else
    {

#line 586
        _S19 = froxel_0 >= ((&kernelContext_6)->params_0->froxel_count_0);

#line 586
    }

#line 586
    if(_S19)
    {
        return;
    }

    uint tile_x_1 = froxel_0 % max(params_1->grid_x_0, 1U);
    uint _S20 = froxel_0 / max(params_1->grid_x_0, 1U);

#line 592
    uint tile_y_1 = _S20 % max(params_1->grid_y_0, 1U);
    uint slice_0 = froxel_0 / tiles_0;

    thread float3 near_point_1;
    thread float near_depth_1;

#line 596
    volumetric_tile_ray_0(tile_x_1, tile_y_1, &near_point_1, &near_depth_1, &kernelContext_6);

    float3 along_0 = (near_point_1 - (&kernelContext_6)->params_0->eye_0.xyz) / float3(near_depth_1) ;

#line 598
    float from_depth_0;

#line 608
    if(slice_0 == 0U)
    {

#line 608
        from_depth_0 = 0.0f;

#line 608
    }
    else
    {

#line 608
        from_depth_0 = volumetric_slice_start_0(slice_0);

#line 608
    }
    uint _S21 = slice_0 + 1U;

#line 609
    float to_depth_0;

#line 609
    if(_S21 == _S18)
    {

#line 609
        to_depth_0 = 1000.0f;

#line 609
    }
    else
    {

#line 609
        to_depth_0 = volumetric_slice_start_0(_S21);

#line 609
    }

    float3 from_1 = (&kernelContext_6)->params_0->eye_0.xyz + along_0 * float3(from_depth_0) ;
    float3 to_1 = (&kernelContext_6)->params_0->eye_0.xyz + along_0 * float3(to_depth_0) ;

#line 612
    float _S22 = volumetric_sun_visibility_0((from_1 + to_1) * float3(0.5f) , float2(float(tile_x_1), float(tile_y_1)), &kernelContext_6);

#line 627
    *((&kernelContext_6)->visibilities_0+froxel_0) = _S22;

#line 627
    packed_float4 device* _S23 = (&kernelContext_6)->volumetrics_0+froxel_0;

#line 627
    float4 _S24 = volumetric_slice_0(from_1, to_1, _S22, &kernelContext_6);

#line 627
    *_S23 = packed_float4(_S24) ;

    return;
}


#line 641
[[kernel]] void integrateMain(uint3 thread_1 [[thread_position_in_grid]], VolumetricParams_natural_0 constant* params_2 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(0)]], sampler shadow_sampler_2 [[sampler(0)]], float device* visibilities_2 [[buffer(2)]], packed_float4 device* volumetrics_2 [[buffer(1)]])
{

#line 641
    thread KernelContext_0 kernelContext_7;

#line 641
    (&kernelContext_7)->params_0 = params_2;

#line 641
    (&kernelContext_7)->shadow_atlas_0 = shadow_atlas_2;

#line 641
    (&kernelContext_7)->shadow_sampler_0 = shadow_sampler_2;

#line 641
    (&kernelContext_7)->visibilities_0 = visibilities_2;

#line 641
    (&kernelContext_7)->volumetrics_0 = volumetrics_2;

    uint tile_2 = thread_1.x;
    uint tiles_1 = max(params_2->grid_x_0, 1U) * max(params_2->grid_y_0, 1U);
    if(tile_2 >= tiles_1)
    {
        return;
    }
    uint _S25 = max((&kernelContext_7)->params_0->slices_0, 1U);

    float3 _S26 = float3(0.0f, 0.0f, 0.0f);

#line 651
    uint slice_1 = 0U;

#line 651
    float3 accumulated_0 = _S26;

#line 651
    float through_0 = 1.0f;

    for(;;)
    {

#line 653
        if(slice_1 < _S25)
        {
        }
        else
        {

#line 653
            break;
        }
        uint froxel_1 = tile_2 + slice_1 * tiles_1;
        if(froxel_1 >= ((&kernelContext_7)->params_0->froxel_count_0))
        {
            break;
        }

#line 658
        float4 _S27 = float4(*((&kernelContext_7)->volumetrics_0+froxel_1)) ;

#line 658
        *((&kernelContext_7)->volumetrics_0+froxel_1) = packed_float4(float4(accumulated_0, through_0)) ;



        float3 accumulated_1 = accumulated_0 + float3(through_0)  * _S27.xyz;
        float through_1 = through_0 * _S27.w;

#line 653
        slice_1 = slice_1 + 1U;

#line 653
        accumulated_0 = accumulated_1;

#line 653
        through_0 = through_1;

#line 653
    }

#line 665
    return;
}

