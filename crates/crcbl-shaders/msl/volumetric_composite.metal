#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 120 "shaders/volumetric_composite.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 115
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

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


#line 144 "shaders/volumetric_composite.slang"
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


#line 144
struct KernelContext_0
{
    texture2d<float, access::sample> scene_color_0;
    VolumetricParams_natural_0 constant* params_0;
    depth2d<float, access::sample> scene_depth_0;
    packed_float4 device* aerial_0;
    packed_float4 device* volumetrics_0;
    packed_float4 device* lighting_0;
};


#line 320
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 386
float4 aerial_slice_0(uint x0_0, uint x1_0, float fx_0, uint y0_0, uint y1_0, float fy_0, uint slice_0, KernelContext_0 thread* kernelContext_1)
{
    uint row0_0 = y0_0 * 32U;
    uint row1_0 = y1_0 * 32U;

#line 389
    float4 _S1 = float4((1.0f - fx_0)) ;

#line 389
    float4 _S2 = float4(fx_0) ;

#line 394
    return (float4(*(kernelContext_1->aerial_0+((row0_0 + x0_0) * 32U + slice_0)))  * _S1 + float4(*(kernelContext_1->aerial_0+((row0_0 + x1_0) * 32U + slice_0)))  * _S2) * float4((1.0f - fy_0))  + (float4(*(kernelContext_1->aerial_0+((row1_0 + x0_0) * 32U + slice_0)))  * _S1 + float4(*(kernelContext_1->aerial_0+((row1_0 + x1_0) * 32U + slice_0)))  * _S2) * float4(fy_0) ;
}


#line 410
float4 aerial_at_0(float up_0, float azimuth_cosine_0, float distance_0, KernelContext_0 thread* kernelContext_2)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_0 = clamp(up_0, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_0));

#line 414
    float _S3;
    if(clamped_0 >= 0.0f)
    {

#line 415
        _S3 = root_0;

#line 415
    }
    else
    {

#line 415
        _S3 = - root_0;

#line 415
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 31.0f;
    float x0_1 = clamp(floor(across_0), 0.0f, 31.0f);
    float _S4 = min(x0_1 + 1.0f, 31.0f);
    float fx_1 = clamp(across_0 - x0_1, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S3, 0.0f, 1.0f) * 31.0f;
    float y0_1 = clamp(floor(down_0), 0.0f, 31.0f);
    float _S5 = min(y0_1 + 1.0f, 31.0f);
    float fy_1 = clamp(down_0 - y0_1, 0.0f, 1.0f);

    float depth_1 = clamp(distance_0 / 1000.0f, 0.0f, 1.0f) * 32.0f;
    float _S6 = min(floor(depth_1), 31.0f);
    float fz_0 = clamp(depth_1 - _S6, 0.0f, 1.0f);



    float4 _S7 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 433
    float4 closer_0;
    if(_S6 > 0.0f)
    {

#line 434
        float4 _S8 = aerial_slice_0(uint(x0_1), uint(_S4), fx_1, uint(y0_1), uint(_S5), fy_1, uint(_S6) - 1U, kernelContext_2);

#line 434
        closer_0 = _S8;

#line 434
    }
    else
    {

#line 434
        closer_0 = _S7;

#line 434
    }

#line 434
    float4 _S9 = aerial_slice_0(uint(x0_1), uint(_S4), fx_1, uint(y0_1), uint(_S5), fy_1, uint(_S6), kernelContext_2);

#line 439
    return closer_0 * float4((1.0f - fz_0))  + _S9 * float4(fz_0) ;
}


#line 446
float4 aerial_along_0(float3 direction_0, float distance_1, KernelContext_0 thread* kernelContext_3)
{
    float3 sun_0 = kernelContext_3->params_0->aerial_params_0.xyz;
    float _S10 = direction_0.x;

#line 449
    float _S11 = direction_0.z;

#line 449
    float view_flat_0 = sqrt(_S10 * _S10 + _S11 * _S11);
    float _S12 = sun_0.x;

#line 450
    float _S13 = sun_0.z;

#line 450
    float sun_flat_0 = sqrt(_S12 * _S12 + _S13 * _S13);

#line 450
    bool _S14;

    if(view_flat_0 > 0.0f)
    {

#line 452
        _S14 = sun_flat_0 > 0.0f;

#line 452
    }
    else
    {

#line 452
        _S14 = false;

#line 452
    }

#line 452
    float cosine_0;

#line 452
    if(_S14)
    {

#line 452
        cosine_0 = (_S10 * _S12 + _S11 * _S13) / (view_flat_0 * sun_flat_0);

#line 452
    }
    else
    {

#line 452
        cosine_0 = 1.0f;

#line 452
    }

#line 452
    float4 _S15 = aerial_at_0(direction_0.y, cosine_0, distance_1, kernelContext_3);



    return _S15;
}


#line 269
float fog_exp_neg_0(float x_0)
{
    float clamped_1 = clamp(x_0, -87.0f, 87.0f);

    float n_0 = floor(clamped_1 * 1.4426950216293335f + 0.5f);


    float _S16 = - (clamped_1 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 276
    float kernel_0 = 0.0001984127011383f;

#line 276
    int term_0 = int(6);

    for(;;)
    {

#line 278
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 278
            break;
        }
        float _S17 = kernel_0 * _S16 + FOG_KERNEL_0[term_0];

#line 278
        int term_1 = term_0 - int(1);

#line 278
        kernel_0 = _S17;

#line 278
        term_0 = term_1;

#line 278
    }

#line 283
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}



float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S18 = - d_0;

#line 292
        float series_0 = 0.00833333376795053f;

#line 292
        int term_2 = int(3);

        for(;;)
        {

#line 294
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 294
                break;
            }
            float _S19 = series_0 * _S18 + FOG_RATIO_KERNEL_0[term_2];

#line 294
            int term_3 = term_2 - int(1);

#line 294
            series_0 = _S19;

#line 294
            term_2 = term_3;

#line 294
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}



float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 316
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 352
float volumetric_phase_0(float g_0, float cos_theta_0)
{
    float a_0 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    float _S20 = a_0 * a_0;

#line 355
    float d_1 = 1.0f + _S20 - 2.0f * a_0 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S20) / (d_1 * sqrt(d_1));
}


#line 374
float3 volumetric_source_0(float3 view_direction_0, float4 lit_0, KernelContext_0 thread* kernelContext_4)
{



    return kernelContext_4->params_0->fog_color_0.xyz + kernelContext_4->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_4->params_0->sun_direction_0.w, dot(kernelContext_4->params_0->sun_direction_0.xyz, view_direction_0)))  * float3(lit_0.w)  + lit_0.xyz;
}


#line 379
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 379
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 470
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S21 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> scene_color_1 [[texture(1)]], VolumetricParams_natural_0 constant* params_1 [[buffer(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], packed_float4 device* aerial_1 [[buffer(3)]], packed_float4 device* volumetrics_1 [[buffer(1)]], packed_float4 device* lighting_1 [[buffer(2)]])
{

#line 470
    bool _S22;

#line 470
    thread KernelContext_0 kernelContext_5;

#line 470
    (&kernelContext_5)->scene_color_0 = scene_color_1;

#line 470
    (&kernelContext_5)->params_0 = params_1;

#line 470
    (&kernelContext_5)->scene_depth_0 = scene_depth_1;

#line 470
    (&kernelContext_5)->aerial_0 = aerial_1;

#line 470
    (&kernelContext_5)->volumetrics_0 = volumetrics_1;

#line 470
    (&kernelContext_5)->lighting_0 = lighting_1;

    int2 _S23 = int2(position_0.xy);
    int3 _S24 = int3(_S23, int(0));

#line 473
    float4 scene_0 = ((scene_color_1).read(vec<uint,2>(((_S24)).xy), uint(((_S24)).z)));

    uint _S25 = max(params_1->grid_x_0, 1U);
    uint _S26 = max(params_1->grid_y_0, 1U);
    uint _S27 = max(params_1->slices_0, 1U);
    uint tiles_0 = _S25 * _S26;
    uint _S28 = max(params_1->tile_pixels_0, 1U);



    int _S29 = _S23.x;
    int _S30 = _S23.y;

#line 482
    float2 ndc_1 = float2((float(_S29) + 0.5f) / float(max(params_1->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - (float(_S30) + 0.5f) / float(max(params_1->viewport_y_0, 1U)) * 2.0f);

#line 490
    float _S31 = ((scene_depth_1).read(vec<uint,2>(((_S24)).xy), uint(((_S24)).z)));

    bool _S32 = _S31 > 0.0f;

#line 492
    float view_depth_0;

#line 492
    if(_S32)
    {

#line 492
        float3 _S33 = volumetric_unproject_0(ndc_1, _S31, &kernelContext_5);

#line 492
        view_depth_0 = dot((&kernelContext_5)->params_0->depth_row_0, float4(_S33, 1.0f));

#line 492
    }
    else
    {

#line 492
        view_depth_0 = 1000.0f;

#line 492
    }

#line 497
    float view_depth_1 = clamp(view_depth_0, 0.0f, 1000.0f);

#line 497
    float slice_start_0 = 0.0f;

#line 497
    uint slice_1 = 0U;

#line 497
    float next_start_0 = 0.14677993953227997f;

#line 516
    for(;;)
    {

#line 516
        uint _S34 = slice_1 + 1U;

#line 516
        if(_S34 < _S27)
        {

#line 516
            _S22 = next_start_0 <= view_depth_1;

#line 516
        }
        else
        {

#line 516
            _S22 = false;

#line 516
        }

#line 516
        if(_S22)
        {
        }
        else
        {

#line 516
            break;
        }

        float next_start_1 = next_start_0 * 1.46779930591583252f;

#line 519
        slice_start_0 = next_start_0;

#line 519
        next_start_0 = next_start_1;

#line 519
        slice_1 = _S34;

#line 516
    }

#line 527
    uint _S35 = uint(max(_S29, int(0))) / _S28;

#line 527
    uint _S36 = min(_S35, _S25 - 1U);
    uint _S37 = uint(max(_S30, int(0))) / _S28;
    uint froxel_0 = _S36 + min(_S37, _S26 - 1U) * _S25 + slice_1 * tiles_0;

#line 536
    float3 surface_0 = scene_0.xyz;
    if(((&kernelContext_5)->params_0->aerial_params_0.w) > 0.0f)
    {

#line 537
        _S22 = _S32;

#line 537
    }
    else
    {

#line 537
        _S22 = false;

#line 537
    }

#line 537
    float3 surface_1;

#line 537
    if(_S22)
    {

#line 537
        float3 _S38 = volumetric_unproject_0(ndc_1, _S31, &kernelContext_5);

        float3 offset_0 = _S38 - (&kernelContext_5)->params_0->eye_0.xyz;
        float distance_3 = length(offset_0);
        if(distance_3 > 0.0f)
        {

#line 541
            surface_1 = offset_0 / float3(distance_3) ;

#line 541
        }
        else
        {

#line 541
            surface_1 = float3(0.0f, 1.0f, 0.0f);

#line 541
        }

#line 541
        float4 _S39 = aerial_along_0(surface_1, distance_3, &kernelContext_5);

#line 541
        surface_1 = surface_0 * float3(_S39.w)  + _S39.xyz;

#line 537
    }
    else
    {

#line 537
        surface_1 = surface_0;

#line 537
    }

#line 549
    if(((&kernelContext_5)->params_0->fog_params_0.w) > 0.0f)
    {

#line 549
        _S22 = true;

#line 549
    }
    else
    {

#line 549
        _S22 = froxel_0 >= ((&kernelContext_5)->params_0->froxel_count_0);

#line 549
    }

#line 549
    if(_S22)
    {

#line 549
        pixelOutput_0 _S40 = { float4(surface_1, scene_0.w) };

        return _S40;
    }

#line 551
    float4 _S41 = float4(*((&kernelContext_5)->volumetrics_0+froxel_0)) ;

#line 551
    float3 _S42 = volumetric_unproject_0(ndc_1, 1.0f, &kernelContext_5);

#line 561
    float3 along_0 = (_S42 - (&kernelContext_5)->params_0->eye_0.xyz) / float3(max(dot((&kernelContext_5)->params_0->depth_row_0, float4(_S42, 1.0f)), 9.99999997475242708e-07f)) ;
    float3 from_0 = (&kernelContext_5)->params_0->eye_0.xyz + along_0 * float3(slice_start_0) ;
    float3 to_0 = (&kernelContext_5)->params_0->eye_0.xyz + along_0 * float3(max(view_depth_1, slice_start_0)) ;

    float reference_0 = (&kernelContext_5)->params_0->fog_params_0.z;
    float3 segment_0 = to_0 - from_0;
    float length_of_0 = length(segment_0);


    float partial_survives_0 = fog_exp_neg_0(fog_optical_depth_0((&kernelContext_5)->params_0->fog_params_0.x, (&kernelContext_5)->params_0->fog_params_0.y, from_0.y - reference_0, to_0.y - reference_0, length_of_0));

#line 570
    float3 view_direction_1;

#line 576
    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 576
        view_direction_1 = segment_0 / float3(length_of_0) ;

#line 576
    }
    else
    {

#line 576
        view_direction_1 = float3(0.0f, 0.0f, 1.0f);

#line 576
    }

#line 576
    float3 _S43 = volumetric_source_0(view_direction_1, float4(*((&kernelContext_5)->lighting_0+froxel_0)) , &kernelContext_5);

#line 585
    float _S44 = _S41.w;

#line 585
    pixelOutput_0 _S45 = { float4(surface_1 * float3((_S44 * partial_survives_0))  + _S41.xyz + float3(_S44)  * (_S43 * float3((1.0f - partial_survives_0)) ), scene_0.w) };

    return _S45;
}


#line 587
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 261
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 261
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> scene_color_2 [[texture(1)]], VolumetricParams_natural_0 constant* params_2 [[buffer(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], packed_float4 device* aerial_2 [[buffer(3)]], packed_float4 device* volumetrics_2 [[buffer(1)]], packed_float4 device* lighting_2 [[buffer(2)]])
{

#line 261
    thread KernelContext_0 kernelContext_6;

#line 261
    (&kernelContext_6)->scene_color_0 = scene_color_2;

#line 261
    (&kernelContext_6)->params_0 = params_2;

#line 261
    (&kernelContext_6)->scene_depth_0 = scene_depth_2;

#line 261
    (&kernelContext_6)->aerial_0 = aerial_2;

#line 261
    (&kernelContext_6)->volumetrics_0 = volumetrics_2;

#line 261
    (&kernelContext_6)->lighting_0 = lighting_2;

#line 462
    thread FullscreenOutput_0 output_1;

    float2 _S46 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 464
    (&output_1)->uv_2 = _S46;
    (&output_1)->position_2 = float4(_S46 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 465
    thread vertexMain_Result_0 _S47;

#line 465
    (&_S47)->position_1 = output_1.position_2;

#line 465
    (&_S47)->uv_1 = output_1.uv_2;

#line 465
    return _S47;
}

