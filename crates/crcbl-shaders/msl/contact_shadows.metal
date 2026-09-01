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
struct ContactShadowParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    float4 to_light_0;
};


#line 1084
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    ContactShadowParams_natural_0 constant* camera_0;
};


#line 173 "shaders/contact_shadows.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 176
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 173
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 176
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 194
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 194
float2 unproject_z_1(float depth_1, KernelContext_0 thread* kernelContext_3)
{
    return float2((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].z * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].w * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 225
float4 unproject_0(float2 ndc_0, float depth_2, KernelContext_0 thread* kernelContext_4)
{

#line 225
    float2 _S3 = unproject_z_0(depth_2, kernelContext_4);


    return float4((&kernelContext_4->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].y, _S3.x, _S3.y);
}


#line 235
float3 view_position_0(int2 pixel_2, float depth_3, float2 extent_2, KernelContext_0 thread* kernelContext_5)
{

#line 235
    float4 _S4 = unproject_0(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_3, kernelContext_5);

#line 246
    return _S4.xyz / float3(_S4.w) ;
}


#line 235
float3 view_position_1(int2 pixel_3, float depth_4, float2 extent_3, KernelContext_0 thread* kernelContext_6)
{

#line 235
    float4 _S5 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_4, kernelContext_6);

#line 246
    return _S5.xyz / float3(_S5.w) ;
}

float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_7)
{
    int2 _S6 = pixel_4 + int2(int(-1), int(0));

#line 251
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_7);

#line 251
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_7);
    int2 _S9 = pixel_4 + int2(int(1), int(0));

#line 252
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_7);

#line 252
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_7);
    int2 _S12 = pixel_4 + int2(int(0), int(-1));

#line 253
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_7);

#line 253
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_7);
    int2 _S15 = pixel_4 + int2(int(0), int(1));

#line 254
    float _S16 = depth_at_1(_S15, extent_4, kernelContext_7);

#line 254
    float3 _S17 = view_position_1(_S15, _S16, size_0, kernelContext_7);

    float _S18 = centre_0.z;

#line 256
    float3 horizontal_0;
    if((abs(_S11.z - _S18)) < (abs(_S18 - _S8.z)))
    {

#line 257
        horizontal_0 = _S11 - centre_0;

#line 257
    }
    else
    {

#line 257
        horizontal_0 = centre_0 - _S8;

#line 257
    }

#line 257
    float3 vertical_0;


    if((abs(_S17.z - _S18)) < (abs(_S18 - _S14.z)))
    {

#line 260
        vertical_0 = _S17 - centre_0;

#line 260
    }
    else
    {

#line 260
        vertical_0 = centre_0 - _S14;

#line 260
    }

#line 270
    return normalize(cross(vertical_0, horizontal_0));
}

float2 pixel_of_0(float2 ndc_1, float2 size_1)
{
    return float2((ndc_1.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_1.y * 0.5f) * size_1.y);
}

float2 ndc_of_0(float2 at_0, float2 size_2)
{
    return float2(at_0.x / size_2.x * 2.0f - 1.0f, 1.0f - at_0.y / size_2.y * 2.0f);
}


#line 294
float cell_exit_0(float2 at_1, float2 forward_0, float size_3, float reach_0)
{

    float _S19 = forward_0.x;

#line 297
    bool _S20 = _S19 > 0.0f;

#line 297
    float along_x_0;

#line 297
    if(_S20)
    {

#line 297
        along_x_0 = (floor(at_1.x / size_3) + 1.0f) * size_3;

#line 297
    }
    else
    {

#line 297
        along_x_0 = floor(at_1.x / size_3) * size_3;

#line 297
    }
    float _S21 = forward_0.y;

#line 298
    bool _S22 = _S21 > 0.0f;

#line 298
    float along_y_0;

#line 298
    if(_S22)
    {

#line 298
        along_y_0 = (floor(at_1.y / size_3) + 1.0f) * size_3;

#line 298
    }
    else
    {

#line 298
        along_y_0 = floor(at_1.y / size_3) * size_3;

#line 298
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 299
    float _S23;

    if((abs(_S19)) < 9.99999997475242708e-07f)
    {

#line 301
        along_x_0 = reach_0;

#line 301
    }
    else
    {

#line 302
        if(_S20)
        {

#line 302
            _S23 = nudge_0;

#line 302
        }
        else
        {

#line 302
            _S23 = - nudge_0;

#line 302
        }

#line 302
        along_x_0 = (along_x_0 + _S23 - at_1.x) / _S19;

#line 301
    }


    if((abs(_S21)) < 9.99999997475242708e-07f)
    {

#line 304
        along_y_0 = reach_0;

#line 304
    }
    else
    {

#line 305
        if(_S22)
        {

#line 305
            _S23 = nudge_0;

#line 305
        }
        else
        {

#line 305
            _S23 = - nudge_0;

#line 305
        }

#line 305
        along_y_0 = (along_y_0 + _S23 - at_1.y) / _S21;

#line 304
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 288
float view_z_of_0(float depth_5, KernelContext_0 thread* kernelContext_8)
{

#line 288
    float2 _S24 = unproject_z_1(depth_5, kernelContext_8);


    return _S24.x / _S24.y;
}


#line 283
float thickness_at_0(float advance_0, float depth_6)
{
    return max(advance_0, abs(depth_6) * 0.01999999955296516f);
}


#line 285
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 285
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 319
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S25 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], ContactShadowParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 319
    thread KernelContext_0 kernelContext_9;

#line 319
    (&kernelContext_9)->scene_depth_0 = scene_depth_1;

#line 319
    (&kernelContext_9)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_5 = int2(int(width_0), int(height_0));
    float _S26 = float(width_0);

#line 325
    float _S27 = float(height_0);

#line 325
    float2 size_4 = float2(_S26, _S27);
    int2 _S28 = int2(position_0.xy);

#line 326
    float _S29 = depth_at_0(_S28, extent_5, &kernelContext_9);


    if(_S29 <= 0.0f)
    {

#line 329
        pixelOutput_0 _S30 = { 1.0f };



        return _S30;
    }

#line 333
    float3 _S31 = view_position_0(_S28, _S29, size_4, &kernelContext_9);

#line 333
    float3 _S32 = normal_at_0(_S28, _S31, extent_5, size_4, &kernelContext_9);

#line 338
    float3 ray_0 = (&kernelContext_9)->camera_0->to_light_0.xyz;

    float facing_0 = saturate(dot(_S32, ray_0) / 0.10000000149011612f);
    if(facing_0 <= 0.0f)
    {

#line 341
        pixelOutput_0 _S33 = { 1.0f };

        return _S33;
    }

    float _S34 = _S31.z;

#line 346
    float3 start_0 = _S31 + _S32 * float3((abs(_S34) * 0.00499999988824129f)) ;
    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((float4(ray_0, 0.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S35 = clip_start_0.w;

#line 349
    if(_S35 <= 0.0f)
    {

#line 349
        pixelOutput_0 _S36 = { 1.0f };

        return _S36;
    }

    float2 _S37 = clip_start_0.xy;

#line 354
    float2 _S38 = float2(_S35) ;

#line 354
    float2 at_start_0 = pixel_of_0(_S37 / _S38, size_4);

    float _S39 = clip_ray_0.w;

#line 356
    float2 ndc_rate_0 = (clip_ray_0.xy * _S38 - _S37 * float2(_S39) ) / float2((_S35 * _S35)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S26, - ndc_rate_0.y * 0.5f * _S27);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 359
        pixelOutput_0 _S40 = { 1.0f };



        return _S40;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 372
    float4 clip_end_0 = (((float4(start_0 + ray_0 * float3(0.25f) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S41 = clip_end_0.w;

#line 373
    float travel_0;

#line 373
    if(_S41 > 0.0f)
    {

#line 373
        travel_0 = min(15.0f, max(dot(pixel_of_0(clip_end_0.xy / float2(_S41) , size_4) - at_start_0, forward_1), 0.0f));

#line 373
    }
    else
    {

#line 373
        travel_0 = 15.0f;

#line 373
    }

#line 378
    float _S42 = forward_1.x;

#line 378
    if(_S42 > 0.0f)
    {

#line 378
        travel_0 = min(travel_0, (_S26 - 1.0f - at_start_0.x) / _S42);

#line 378
    }
    else
    {

        if(_S42 < 0.0f)
        {

#line 382
            travel_0 = min(travel_0, - at_start_0.x / _S42);

#line 382
        }

#line 378
    }

#line 386
    float _S43 = forward_1.y;

#line 386
    if(_S43 > 0.0f)
    {

#line 386
        travel_0 = min(travel_0, (_S27 - 1.0f - at_start_0.y) / _S43);

#line 386
    }
    else
    {

        if(_S43 < 0.0f)
        {

#line 390
            travel_0 = min(travel_0, - at_start_0.y / _S43);

#line 390
        }

#line 386
    }

#line 394
    if(_S39 < 0.0f)
    {


        float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));


        float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S35) / _S39)) ;

#line 401
        travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 394
    }

#line 405
    float _S44 = max(travel_0, 0.0f);
    if(_S44 < 2.0f)
    {

#line 406
        pixelOutput_0 _S45 = { 1.0f };



        return _S45;
    }


    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S44) , size_4);

#line 414
    float when_end_0;

    if((abs(_S42)) >= (abs(_S43)))
    {

#line 416
        float _S46 = ndc_end_0.x;

#line 416
        when_end_0 = (_S46 * _S35 - clip_start_0.x) / (clip_ray_0.x - _S46 * _S39);

#line 416
    }
    else
    {

#line 417
        float _S47 = ndc_end_0.y;

#line 417
        when_end_0 = (_S47 * _S35 - clip_start_0.y) / (clip_ray_0.y - _S47 * _S39);

#line 416
    }

#line 416
    bool _S48;

    if(!(when_end_0 > 0.0f))
    {

#line 418
        _S48 = true;

#line 418
    }
    else
    {

#line 418
        _S48 = !isfinite(when_end_0);

#line 418
    }

#line 418
    if(_S48)
    {

#line 418
        pixelOutput_0 _S49 = { 1.0f };

        return _S49;
    }

    float inverse_w_start_0 = 1.0f / _S35;

    float inverse_w_end_0 = 1.0f / (_S35 + when_end_0 * _S39);
    float _S50 = start_0.z;

#line 426
    float _S51 = _S50 * inverse_w_start_0;
    float _S52 = (_S50 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 435
    float _S53 = _S50 - _S34;

#line 435
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S44), _S44);

#line 435
    float previous_gap_0 = _S53;

#line 435
    float entry_z_0 = _S50;

#line 435
    uint step_0 = 0U;
    for(;;)
    {

#line 436
        if(step_0 < 16U)
        {
        }
        else
        {

#line 436
            break;
        }
        float2 at_2 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S54 = min(at_travel_0 + cell_exit_0(at_2, forward_1, 1.0f, _S44), _S44);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S54) ;
        float along_0 = _S54 / _S44;

        float exit_z_0 = mix(_S51, _S52, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 443
        float _S55 = depth_at_0(int2(floor(at_2)), extent_5, &kernelContext_9);

#line 443
        float gap_0;



        if(_S55 <= 0.0f)
        {

#line 447
            gap_0 = 1.0f;

#line 447
        }
        else
        {

#line 447
            float _S56 = view_z_of_0(_S55, &kernelContext_9);

#line 447
            gap_0 = exit_z_0 - _S56;

#line 447
        }
        if(gap_0 <= 0.0f)
        {

#line 448
            _S48 = previous_gap_0 > 0.0f;

#line 448
        }
        else
        {

#line 448
            _S48 = false;

#line 448
        }

#line 448
        if(_S48)
        {
            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {


                float2 hit_ndc_0 = ndc_of_0(mix(at_2, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) ), size_4);

#line 456
                pixelOutput_0 _S57 = { saturate(1.0f - facing_0 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S54 / _S44) / 0.25f) * saturate(1.0f - behind_0 / thickness_0)) };



                return _S57;
            }

#line 448
        }

#line 466
        if(_S54 >= _S44)
        {
            break;
        }

#line 436
        uint step_1 = step_0 + 1U;

#line 436
        at_travel_0 = _S54;

#line 436
        previous_gap_0 = gap_0;

#line 436
        entry_z_0 = exit_z_0;

#line 436
        step_0 = step_1;

#line 436
    }

#line 436
    pixelOutput_0 _S58 = { 1.0f };

#line 471
    return _S58;
}


#line 471
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 167
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 167
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], ContactShadowParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 167
    thread KernelContext_0 kernelContext_10;

#line 167
    (&kernelContext_10)->scene_depth_0 = scene_depth_2;

#line 167
    (&kernelContext_10)->camera_0 = camera_2;

#line 312
    thread FullscreenOutput_0 output_1;
    float2 _S59 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 313
    (&output_1)->uv_2 = _S59;
    (&output_1)->position_2 = float4(_S59 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 314
    thread vertexMain_Result_0 _S60;

#line 314
    (&_S60)->position_1 = output_1.position_2;

#line 314
    (&_S60)->uv_1 = output_1.uv_2;

#line 314
    return _S60;
}

