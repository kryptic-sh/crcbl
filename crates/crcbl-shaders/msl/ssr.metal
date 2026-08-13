#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 90
struct SsrParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
};


#line 1084
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    texture2d<float, access::sample> reflectivity_0;
    SsrParams_natural_0 constant* camera_0;
    texture2d<float, access::sample> scene_color_0;
};


#line 277 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 280
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 277
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 280
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 289
float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 299
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 289
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 299
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 315
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 317
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 317
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 318
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 318
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 319
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 319
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 320
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 320
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 322
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 323
        horizontal_0 = _S8 - centre_0;

#line 323
    }
    else
    {

#line 323
        horizontal_0 = centre_0 - _S5;

#line 323
    }

#line 323
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 326
        vertical_0 = _S14 - centre_0;

#line 326
    }
    else
    {

#line 326
        vertical_0 = centre_0 - _S11;

#line 326
    }

#line 336
    return normalize(cross(vertical_0, horizontal_0));
}


#line 345
float2 pixel_of_0(float2 ndc_0, float2 size_1)
{
    return float2((ndc_0.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_0.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_0, float2 size_2)
{
    return float2(at_0.x / size_2.x * 2.0f - 1.0f, 1.0f - at_0.y / size_2.y * 2.0f);
}


#line 364
float thickness_at_0(float advance_0, float depth_2)
{
    return max(advance_0, abs(depth_2) * 0.01999999955296516f);
}


#line 366
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 366
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 381
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S16 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 381
    float3 reflection_0;

#line 381
    thread KernelContext_0 kernelContext_5;

#line 381
    (&kernelContext_5)->scene_depth_0 = scene_depth_1;

#line 381
    (&kernelContext_5)->reflectivity_0 = reflectivity_1;

#line 381
    (&kernelContext_5)->camera_0 = camera_1;

#line 381
    (&kernelContext_5)->scene_color_0 = scene_color_1;

    thread uint width_0;
    thread uint height_0;



    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_5 = int2(int(width_0), int(height_0));
    float _S17 = float(width_0);

#line 390
    float _S18 = float(height_0);

#line 390
    float2 size_3 = float2(_S17, _S18);
    int2 _S19 = int2(position_0.xy);

#line 398
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S20 = int3(_S19, int(0));

#line 400
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S20)).xy), uint(((_S20)).z)));
    float sharpness_0 = saturate(1.0f - surface_0.w / 0.5f);

#line 401
    float _S21 = depth_at_0(_S19, extent_5, &kernelContext_5);

#line 401
    bool _S22;

#line 406
    if(_S21 <= 0.0f)
    {

#line 406
        _S22 = true;

#line 406
    }
    else
    {

#line 406
        _S22 = sharpness_0 <= 0.0f;

#line 406
    }

#line 406
    if(_S22)
    {

#line 406
        pixelOutput_0 _S23 = { NOTHING_0 };

        return _S23;
    }

#line 408
    float3 _S24 = view_position_0(_S19, _S21, size_3, &kernelContext_5);

#line 408
    float3 _S25 = normal_at_0(_S19, _S24, extent_5, size_3, &kernelContext_5);

#line 414
    float3 towards_0 = normalize(_S24);
    float3 ray_0 = reflect(towards_0, _S25);

#line 420
    float3 _S26 = - towards_0;
    float3 f0_0 = surface_0.xyz;
    float grazing_0 = 1.0f - saturate(dot(_S25, _S26));
    float grazing2_0 = grazing_0 * grazing_0;
    float3 _S27 = f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) ;


    float facing_0 = saturate((1.0f - dot(ray_0, _S26)) / 0.05000000074505806f);
    if(facing_0 <= 0.0f)
    {

#line 428
        pixelOutput_0 _S28 = { NOTHING_0 };

        return _S28;
    }


    float _S29 = _S24.z;

#line 434
    float3 start_0 = _S24 + _S25 * float3((abs(_S29) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_5)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_5)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_5)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_5)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_5)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_5)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_5)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_5)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_5)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_5)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_5)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_5)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_5)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_5)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_5)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_5)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((float4(ray_0, 0.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_5)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_5)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_5)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_5)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_5)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_5)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_5)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_5)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_5)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_5)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_5)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_5)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_5)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_5)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_5)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_5)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S30 = clip_start_0.w;

#line 439
    if(_S30 <= 0.0f)
    {

#line 439
        pixelOutput_0 _S31 = { NOTHING_0 };

        return _S31;
    }
    float2 _S32 = clip_start_0.xy;

#line 443
    float2 _S33 = float2(_S30) ;

#line 443
    float2 at_start_0 = pixel_of_0(_S32 / _S33, size_3);

#line 449
    float2 _S34 = clip_ray_0.xy;

#line 449
    float _S35 = clip_ray_0.w;

#line 449
    float2 _S36 = float2(_S35) ;

#line 449
    float2 ndc_rate_0 = (_S34 * _S33 - _S32 * _S36) / float2((_S30 * _S30)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S17, - ndc_rate_0.y * 0.5f * _S18);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 452
        pixelOutput_0 _S37 = { NOTHING_0 };

        return _S37;
    }
    float2 forward_0 = screen_rate_0 / float2(rate_0) ;

#line 463
    float stride_0 = 0.75f * min(_S17, _S18) / 96.0f;
    float travel_0 = 96.0f * stride_0;
    float _S38 = forward_0.x;

#line 465
    float travel_1;

#line 465
    if(_S38 > 0.0f)
    {

#line 465
        travel_1 = min(travel_0, (_S17 - 1.0f - at_start_0.x) / _S38);

#line 465
    }
    else
    {

        if(_S38 < 0.0f)
        {

#line 469
            travel_1 = min(travel_0, - at_start_0.x / _S38);

#line 469
        }
        else
        {

#line 469
            travel_1 = travel_0;

#line 469
        }

#line 465
    }

#line 473
    float _S39 = forward_0.y;

#line 473
    if(_S39 > 0.0f)
    {

#line 473
        travel_1 = min(travel_1, (_S18 - 1.0f - at_start_0.y) / _S39);

#line 473
    }
    else
    {

        if(_S39 < 0.0f)
        {

#line 477
            travel_1 = min(travel_1, - at_start_0.y / _S39);

#line 477
        }

#line 473
    }

#line 485
    if(_S35 > 0.0f)
    {

#line 485
        travel_1 = min(travel_1, max(dot(pixel_of_0(_S34 / _S36, size_3) - at_start_0, forward_0), 0.0f));

#line 485
    }
    else
    {

#line 498
        if(_S35 < 0.0f)
        {

#line 505
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_5)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_5)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 510
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S30) / _S35)) ;

#line 510
            travel_1 = min(travel_1, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_3) - at_start_0, forward_0), 0.0f));

#line 498
        }

#line 485
    }

#line 517
    uint steps_0 = uint(max(travel_1, 0.0f) / stride_0);
    if(steps_0 == 0U)
    {

#line 518
        pixelOutput_0 _S40 = { NOTHING_0 };

        return _S40;
    }
    float _S41 = float(steps_0);

#line 522
    float travel_2 = _S41 * stride_0;

#line 528
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_0 * float2(travel_2) , size_3);

#line 528
    float when_end_0;

    if((abs(_S38)) >= (abs(_S39)))
    {

#line 530
        float _S42 = ndc_end_0.x;

#line 530
        when_end_0 = (_S42 * _S30 - clip_start_0.x) / (clip_ray_0.x - _S42 * _S35);

#line 530
    }
    else
    {

#line 531
        float _S43 = ndc_end_0.y;

#line 531
        when_end_0 = (_S43 * _S30 - clip_start_0.y) / (clip_ray_0.y - _S43 * _S35);

#line 530
    }

#line 538
    if(!(when_end_0 > 0.0f))
    {

#line 538
        pixelOutput_0 _S44 = { NOTHING_0 };

        return _S44;
    }

#line 546
    float inverse_w_start_0 = 1.0f / _S30;

    float inverse_w_end_0 = 1.0f / (_S30 + when_end_0 * _S35);
    float _S45 = start_0.z;

#line 549
    float _S46 = _S45 * inverse_w_start_0;
    float _S47 = (_S45 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 559
    float3 _S48 = float3(0.0f, 0.0f, 0.0f);

#line 559
    float previous_gap_0 = _S45 - _S29;

#line 559
    float previous_z_0 = _S45;

#line 559
    float2 previous_at_0 = at_start_0;

#line 559
    uint step_0 = 1U;
    for(;;)
    {

#line 560
        if(step_0 <= steps_0)
        {
        }
        else
        {

#line 560
            reflection_0 = _S48;

#line 560
            break;
        }
        float _S49 = float(step_0);

#line 562
        float along_0 = _S49 / _S41;
        float2 at_1 = at_start_0 + forward_0 * float2((travel_2 * along_0)) ;
        int2 _S50 = int2(at_1);
        float ray_z_0 = mix(_S46, _S47, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 565
        float _S51 = depth_at_0(_S50, extent_5, &kernelContext_5);

#line 565
        float gap_0;

#line 572
        if(_S51 > 0.0f)
        {

#line 572
            float3 _S52 = view_position_0(_S50, _S51, size_3, &kernelContext_5);

#line 572
            gap_0 = ray_z_0 - _S52.z;

#line 572
        }
        else
        {

#line 572
            gap_0 = 1.0f;

#line 572
        }

#line 580
        if(previous_gap_0 > 0.0f)
        {

#line 580
            _S22 = gap_0 < 0.0f;

#line 580
        }
        else
        {

#line 580
            _S22 = false;

#line 580
        }

#line 580
        if(_S22)
        {
            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(ray_z_0 - previous_z_0), ray_z_0);
            if(behind_0 <= thickness_0)
            {

#line 590
                float2 hit_at_0 = mix(previous_at_0, at_1, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_3);

#line 605
                int3 _S53 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_5 - int2(int(1), int(1))), int(0));

#line 605
                reflection_0 = (((&kernelContext_5)->scene_color_0).read(vec<uint,2>(((_S53)).xy), uint(((_S53)).z))).xyz * _S27 * float3((sharpness_0 * facing_0 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S49 / 96.0f) / 0.25f) * saturate(1.0f - behind_0 / thickness_0))) ;

                break;
            }

#line 580
        }

#line 560
        uint step_1 = step_0 + 1U;

#line 560
        previous_gap_0 = gap_0;

#line 560
        previous_z_0 = ray_z_0;

#line 560
        previous_at_0 = at_1;

#line 560
        step_0 = step_1;

#line 560
    }

#line 560
    pixelOutput_0 _S54 = { float4(reflection_0, sharpness_0) };

#line 622
    return _S54;
}


#line 622
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 265
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 265
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 265
    thread KernelContext_0 kernelContext_6;

#line 265
    (&kernelContext_6)->scene_depth_0 = scene_depth_2;

#line 265
    (&kernelContext_6)->reflectivity_0 = reflectivity_2;

#line 265
    (&kernelContext_6)->camera_0 = camera_2;

#line 265
    (&kernelContext_6)->scene_color_0 = scene_color_2;

#line 372
    thread FullscreenOutput_0 output_1;


    float2 _S55 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 375
    (&output_1)->uv_2 = _S55;
    (&output_1)->position_2 = float4(_S55 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 376
    thread vertexMain_Result_0 _S56;

#line 376
    (&_S56)->position_1 = output_1.position_2;

#line 376
    (&_S56)->uv_1 = output_1.uv_2;

#line 376
    return _S56;
}

