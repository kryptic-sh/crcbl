#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 154 "shaders/ssao_hemisphere.slang"
constant array<float3, int(8)> KERNEL_0 = { float3(0.875f, 0.0f, 0.25f), float3(-0.75f, 0.0f, 0.375f), float3(0.0f, 0.75f, 0.25f), float3(0.0f, -0.625f, 0.5f), float3(0.5f, 0.5f, 0.375f), float3(-0.5f, 0.5f, 0.625f), float3(0.375f, -0.375f, 0.75f), float3(-0.25f, -0.25f, 0.875f) };

#line 182
constant array<float2, int(16)> ROTATIONS_0 = { float2(2.0f, 0.0f), float2(-2.0f, 0.0f), float2(1.0f, 1.0f), float2(-1.0f, -1.0f), float2(0.0f, -2.0f), float2(0.0f, 2.0f), float2(1.0f, -1.0f), float2(-1.0f, 1.0f), float2(1.0f, 2.0f), float2(-1.0f, -2.0f), float2(2.0f, 1.0f), float2(-2.0f, -1.0f), float2(2.0f, -1.0f), float2(-2.0f, 1.0f), float2(1.0f, -2.0f), float2(-1.0f, 2.0f) };

#line 263
int2 full_res_pixel_0(int2 pixel_0)
{
    return pixel_0 * int2(int(2)) ;
}


#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 90
struct SsaoParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    float4 params_0;
};


#line 1084
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    SsaoParams_natural_0 constant* camera_0;
};


#line 274 "shaders/ssao_hemisphere.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 277
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 274
float depth_at_1(int2 pixel_2, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_2, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 277
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 514
float3 encode_bent_0(float3 direction_0)
{

#line 514
    float3 _S3 = float3(0.5f) ;

    return direction_0 * _S3 + _S3;
}


#line 295
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 326
float4 unproject_0(float2 ndc_0, float depth_1, KernelContext_0 thread* kernelContext_3)
{

#line 326
    float2 _S4 = unproject_z_0(depth_1, kernelContext_3);


    return float4((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].y, _S4.x, _S4.y);
}


#line 342
float3 view_position_0(int2 pixel_3, float depth_2, float2 extent_2, KernelContext_0 thread* kernelContext_4)
{

#line 342
    float4 _S5 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_2.y * 2.0f), depth_2, kernelContext_4);

#line 353
    return _S5.xyz / float3(_S5.w) ;
}


#line 342
float3 view_position_1(int2 pixel_4, float depth_3, float2 extent_3, KernelContext_0 thread* kernelContext_5)
{

#line 342
    float4 _S6 = unproject_0(float2((float(pixel_4.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_4.y) + 0.5f) / extent_3.y * 2.0f), depth_3, kernelContext_5);

#line 353
    return _S6.xyz / float3(_S6.w) ;
}


#line 369
float3 normal_at_0(int2 pixel_5, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_6)
{
    int2 _S7 = pixel_5 + int2(int(-1), int(0));

#line 371
    float _S8 = depth_at_1(_S7, extent_4, kernelContext_6);

#line 371
    float3 _S9 = view_position_1(_S7, _S8, size_0, kernelContext_6);
    int2 _S10 = pixel_5 + int2(int(1), int(0));

#line 372
    float _S11 = depth_at_1(_S10, extent_4, kernelContext_6);

#line 372
    float3 _S12 = view_position_1(_S10, _S11, size_0, kernelContext_6);
    int2 _S13 = pixel_5 + int2(int(0), int(-1));

#line 373
    float _S14 = depth_at_1(_S13, extent_4, kernelContext_6);

#line 373
    float3 _S15 = view_position_1(_S13, _S14, size_0, kernelContext_6);
    int2 _S16 = pixel_5 + int2(int(0), int(1));

#line 374
    float _S17 = depth_at_1(_S16, extent_4, kernelContext_6);

#line 374
    float3 _S18 = view_position_1(_S16, _S17, size_0, kernelContext_6);

    float _S19 = centre_0.z;

#line 376
    float3 horizontal_0;
    if((abs(_S12.z - _S19)) < (abs(_S19 - _S9.z)))
    {

#line 377
        horizontal_0 = _S12 - centre_0;

#line 377
    }
    else
    {

#line 377
        horizontal_0 = centre_0 - _S9;

#line 377
    }

#line 377
    float3 vertical_0;


    if((abs(_S18.z - _S19)) < (abs(_S19 - _S15.z)))
    {

#line 380
        vertical_0 = _S18 - centre_0;

#line 380
    }
    else
    {

#line 380
        vertical_0 = centre_0 - _S15;

#line 380
    }

#line 390
    return normalize(cross(vertical_0, horizontal_0));
}


#line 405
float sampling_radius_0(KernelContext_0 thread* kernelContext_7)
{
    float asked_0 = kernelContext_7->camera_0->params_0.x;
    if(asked_0 <= 0.0f)
    {
        return 0.5f;
    }
    return clamp(asked_0, 0.0625f, 4.0f);
}


#line 432
float occlusion_at_0(int2 pixel_6, uint tile_0, float3 centre_1, float3 normal_0, int2 extent_5, float2 size_1, KernelContext_0 thread* kernelContext_8)
{

#line 432
    float _S20 = sampling_radius_0(kernelContext_8);

#line 437
    float _S21 = _S20 * 0.03999999910593033f;

#line 445
    float3 seed_0 = float3(ROTATIONS_0[tile_0], 0.0f);
    float3 tangent_0 = seed_0 - normal_0 * float3(dot(seed_0, normal_0)) ;

#line 446
    float3 across_0;



    if((dot(tangent_0, tangent_0)) > 9.99999993922529029e-09f)
    {

#line 450
        across_0 = normalize(tangent_0);

#line 450
    }
    else
    {

#line 450
        across_0 = float3(1.0f, 0.0f, 0.0f);

#line 450
    }
    float3 _S22 = cross(normal_0, across_0);

#line 451
    uint index_0 = 0U;

#line 451
    float blocked_0 = 0.0f;


    for(;;)
    {

#line 454
        if(index_0 < 8U)
        {
        }
        else
        {

#line 454
            break;
        }

        float3 at_0 = centre_1 + (across_0 * float3(KERNEL_0[index_0].x)  + _S22 * float3(KERNEL_0[index_0].y)  + normal_0 * float3(KERNEL_0[index_0].z) ) * float3(_S20) ;

        float4 clip_0 = (((float4(at_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_8->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_8->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_8->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_8->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_8->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_8->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_8->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_8->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_8->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_8->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_8->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_8->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_8->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_8->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_8->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_8->camera_0->proj_0.data_0[int(3)][int(3)]))));

        float _S23 = clip_0.w;

#line 461
        if(_S23 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 454
            continue;
        }

#line 465
        float2 ndc_1 = clip_0.xy / float2(_S23) ;

        int _S24 = int((ndc_1.x * 0.5f + 0.5f) * size_1.x);
        int _S25 = int((0.5f - ndc_1.y * 0.5f) * size_1.y);

#line 466
        int2 tap_0 = int2(_S24, _S25);

#line 466
        bool _S26;

#line 472
        if(_S24 < int(0))
        {

#line 472
            _S26 = true;

#line 472
        }
        else
        {

#line 472
            _S26 = _S25 < int(0);

#line 472
        }

#line 472
        bool _S27;

#line 472
        if(_S26)
        {

#line 472
            _S27 = true;

#line 472
        }
        else
        {

#line 472
            _S27 = _S24 >= (extent_5.x);

#line 472
        }

#line 472
        bool _S28;

#line 472
        if(_S27)
        {

#line 472
            _S28 = true;

#line 472
        }
        else
        {

#line 472
            _S28 = _S25 >= (extent_5.y);

#line 472
        }

#line 472
        if(_S28)
        {
            index_0 = index_0 + 1U;

#line 454
            continue;
        }

#line 454
        float _S29 = depth_at_0(tap_0, extent_5, kernelContext_8);

#line 478
        if(_S29 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 454
            continue;
        }

#line 454
        float3 _S30 = view_position_0(tap_0, _S29, size_1, kernelContext_8);

#line 487
        float _S31 = _S30.z;

#line 487
        float blocked_1;

#line 487
        if(_S31 >= (at_0.z + _S21))
        {

#line 487
            blocked_1 = blocked_0 + saturate(_S20 / max(abs(centre_1.z - _S31), 0.00000999999974738f));

#line 487
        }
        else
        {

#line 487
            blocked_1 = blocked_0;

#line 487
        }

#line 487
        blocked_0 = blocked_1;

#line 454
        index_0 = index_0 + 1U;

#line 454
    }

#line 496
    return blocked_0 / 8.0f;
}


#line 496
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 496
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 520
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S32 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 520
    thread KernelContext_0 kernelContext_9;

#line 520
    (&kernelContext_9)->scene_depth_0 = scene_depth_1;

#line 520
    (&kernelContext_9)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 530
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_6 = int2(int(width_0), int(height_0));
    float2 size_2 = float2(float(width_0), float(height_0));

#line 540
    int2 _S33 = int2(position_0.xy);
    int2 pixel_7 = full_res_pixel_0(_S33);
    uint tile_1 = (uint(_S33.y) & 3U) * 4U + (uint(_S33.x) & 3U);

#line 542
    float _S34 = depth_at_0(pixel_7, extent_6, &kernelContext_9);



    if(_S34 <= 0.0f)
    {

#line 546
        pixelOutput_0 _S35 = { float4(1.0f, encode_bent_0(float3(0.0f, 0.0f, 0.0f))) };

        return _S35;
    }

#line 548
    float3 _S36 = view_position_0(pixel_7, _S34, size_2, &kernelContext_9);

#line 548
    float3 _S37 = normal_at_0(pixel_7, _S36, extent_6, size_2, &kernelContext_9);

#line 548
    float _S38 = occlusion_at_0(pixel_7, tile_1, _S36, _S37, extent_6, size_2, &kernelContext_9);

#line 548
    pixelOutput_0 _S39 = { float4(saturate(1.0f - _S38), encode_bent_0(float3(0.0f, 0.0f, 0.0f))) };

#line 556
    return _S39;
}


#line 556
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 245
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 245
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 245
    thread KernelContext_0 kernelContext_10;

#line 245
    (&kernelContext_10)->scene_depth_0 = scene_depth_2;

#line 245
    (&kernelContext_10)->camera_0 = camera_2;

#line 502
    thread FullscreenOutput_0 output_1;


    float2 _S40 = float2(float((index_1 << 1U) & 2U), float(index_1 & 2U));

#line 505
    (&output_1)->uv_2 = _S40;
    (&output_1)->position_2 = float4(_S40 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 506
    thread vertexMain_Result_0 _S41;

#line 506
    (&_S41)->position_1 = output_1.position_2;

#line 506
    (&_S41)->uv_1 = output_1.uv_2;

#line 506
    return _S41;
}

