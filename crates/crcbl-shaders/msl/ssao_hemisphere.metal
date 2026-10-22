#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 166 "shaders/ssao_hemisphere.slang"
constant array<float3, int(8)> KERNEL_0 = { { float3(0.875f, 0.0f, 0.25f), float3(-0.75f, 0.0f, 0.375f), float3(0.0f, 0.75f, 0.25f), float3(0.0f, -0.625f, 0.5f), float3(0.5f, 0.5f, 0.375f), float3(-0.5f, 0.5f, 0.625f), float3(0.375f, -0.375f, 0.75f), float3(-0.25f, -0.25f, 0.875f) } };

#line 194
constant array<float2, int(16)> ROTATIONS_0 = { { float2(2.0f, 0.0f), float2(-2.0f, 0.0f), float2(1.0f, 1.0f), float2(-1.0f, -1.0f), float2(0.0f, -2.0f), float2(0.0f, 2.0f), float2(1.0f, -1.0f), float2(-1.0f, 1.0f), float2(1.0f, 2.0f), float2(-1.0f, -2.0f), float2(2.0f, 1.0f), float2(-2.0f, -1.0f), float2(2.0f, -1.0f), float2(-2.0f, 1.0f), float2(1.0f, -2.0f), float2(-1.0f, 2.0f) } };

#line 275
int2 full_res_pixel_0(int2 pixel_0)
{
    return pixel_0 * int2(int(2)) ;
}


#line 113
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 113
struct SsaoParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    float4 params_0;
};


#line 1095 "core"
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    SsaoParams_natural_0 constant* camera_0;
};


#line 286 "shaders/ssao_hemisphere.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 289
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 286
float depth_at_1(int2 pixel_2, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_2, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 289
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 533
float3 encode_bent_0(float3 direction_0)
{

#line 533
    float3 _S3;


    if(all(direction_0 == float3(0.0f, 0.0f, 0.0f)))
    {

#line 536
        _S3 = float3(0.50196081399917603f, 0.50196081399917603f, 0.50196081399917603f);

#line 536
    }
    else
    {

#line 536
        float3 _S4 = float3(0.5f) ;

#line 536
        _S3 = direction_0 * _S4 + _S4;

#line 536
    }

#line 535
    return _S3;
}


#line 307
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 338
float4 unproject_0(float2 ndc_0, float depth_1, KernelContext_0 thread* kernelContext_3)
{

#line 338
    float2 _S5 = unproject_z_0(depth_1, kernelContext_3);


    return float4((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].y, _S5.x, _S5.y);
}


#line 354
float3 view_position_0(int2 pixel_3, float depth_2, float2 extent_2, KernelContext_0 thread* kernelContext_4)
{

#line 354
    float4 _S6 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_2.y * 2.0f), depth_2, kernelContext_4);

#line 365
    return _S6.xyz / float3(_S6.w) ;
}


#line 354
float3 view_position_1(int2 pixel_4, float depth_3, float2 extent_3, KernelContext_0 thread* kernelContext_5)
{

#line 354
    float4 _S7 = unproject_0(float2((float(pixel_4.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_4.y) + 0.5f) / extent_3.y * 2.0f), depth_3, kernelContext_5);

#line 365
    return _S7.xyz / float3(_S7.w) ;
}


#line 381
float3 normal_at_0(int2 pixel_5, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_6)
{
    int2 _S8 = pixel_5 + int2(int(-1), int(0));

#line 383
    float _S9 = depth_at_1(_S8, extent_4, kernelContext_6);

#line 383
    float3 _S10 = view_position_1(_S8, _S9, size_0, kernelContext_6);
    int2 _S11 = pixel_5 + int2(int(1), int(0));

#line 384
    float _S12 = depth_at_1(_S11, extent_4, kernelContext_6);

#line 384
    float3 _S13 = view_position_1(_S11, _S12, size_0, kernelContext_6);
    int2 _S14 = pixel_5 + int2(int(0), int(-1));

#line 385
    float _S15 = depth_at_1(_S14, extent_4, kernelContext_6);

#line 385
    float3 _S16 = view_position_1(_S14, _S15, size_0, kernelContext_6);
    int2 _S17 = pixel_5 + int2(int(0), int(1));

#line 386
    float _S18 = depth_at_1(_S17, extent_4, kernelContext_6);

#line 386
    float3 _S19 = view_position_1(_S17, _S18, size_0, kernelContext_6);

    float _S20 = centre_0.z;

#line 388
    float3 horizontal_0;
    if((abs(_S13.z - _S20)) < (abs(_S20 - _S10.z)))
    {

#line 389
        horizontal_0 = _S13 - centre_0;

#line 389
    }
    else
    {

#line 389
        horizontal_0 = centre_0 - _S10;

#line 389
    }

#line 389
    float3 vertical_0;


    if((abs(_S19.z - _S20)) < (abs(_S20 - _S16.z)))
    {

#line 392
        vertical_0 = _S19 - centre_0;

#line 392
    }
    else
    {

#line 392
        vertical_0 = centre_0 - _S16;

#line 392
    }

#line 402
    return normalize(cross(vertical_0, horizontal_0));
}


#line 417
float sampling_radius_0(KernelContext_0 thread* kernelContext_7)
{
    float asked_0 = kernelContext_7->camera_0->params_0.x;
    if(asked_0 <= 0.0f)
    {
        return 0.5f;
    }
    return clamp(asked_0, 0.0625f, 4.0f);
}


#line 444
float occlusion_at_0(int2 pixel_6, uint tile_0, float3 centre_1, float3 normal_0, int2 extent_5, float2 size_1, KernelContext_0 thread* kernelContext_8)
{

#line 444
    float _S21 = sampling_radius_0(kernelContext_8);

#line 449
    float _S22 = _S21 * 0.03999999910593033f;

#line 457
    float3 seed_0 = float3(ROTATIONS_0[tile_0], 0.0f);
    float3 tangent_0 = seed_0 - normal_0 * float3(dot(seed_0, normal_0)) ;

#line 458
    float3 across_0;



    if((dot(tangent_0, tangent_0)) > 9.99999993922529029e-09f)
    {

#line 462
        across_0 = normalize(tangent_0);

#line 462
    }
    else
    {

#line 462
        across_0 = float3(1.0f, 0.0f, 0.0f);

#line 462
    }
    float3 _S23 = cross(normal_0, across_0);

#line 463
    uint index_0 = 0U;

#line 463
    float blocked_0 = 0.0f;


    for(;;)
    {

#line 466
        if(index_0 < 8U)
        {
        }
        else
        {

#line 466
            break;
        }

        float3 at_0 = centre_1 + (across_0 * float3(KERNEL_0[index_0].x)  + _S23 * float3(KERNEL_0[index_0].y)  + normal_0 * float3(KERNEL_0[index_0].z) ) * float3(_S21) ;

        float4 clip_0 = (((float4(at_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_8->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_8->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_8->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_8->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_8->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_8->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_8->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_8->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_8->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_8->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_8->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_8->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_8->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_8->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_8->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_8->camera_0->proj_0.data_0[int(3)][int(3)]))));

        float _S24 = clip_0.w;

#line 473
        if(_S24 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 466
            continue;
        }

#line 477
        float2 ndc_1 = clip_0.xy / float2(_S24) ;

        int _S25 = int((ndc_1.x * 0.5f + 0.5f) * size_1.x);
        int _S26 = int((0.5f - ndc_1.y * 0.5f) * size_1.y);

#line 478
        int2 tap_0 = int2(_S25, _S26);

#line 478
        bool _S27;

#line 484
        if(_S25 < int(0))
        {

#line 484
            _S27 = true;

#line 484
        }
        else
        {

#line 484
            _S27 = _S26 < int(0);

#line 484
        }

#line 484
        bool _S28;

#line 484
        if(_S27)
        {

#line 484
            _S28 = true;

#line 484
        }
        else
        {

#line 484
            _S28 = _S25 >= (extent_5.x);

#line 484
        }

#line 484
        bool _S29;

#line 484
        if(_S28)
        {

#line 484
            _S29 = true;

#line 484
        }
        else
        {

#line 484
            _S29 = _S26 >= (extent_5.y);

#line 484
        }

#line 484
        if(_S29)
        {
            index_0 = index_0 + 1U;

#line 466
            continue;
        }

#line 466
        float _S30 = depth_at_0(tap_0, extent_5, kernelContext_8);

#line 490
        if(_S30 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 466
            continue;
        }

#line 466
        float3 _S31 = view_position_0(tap_0, _S30, size_1, kernelContext_8);

#line 499
        float _S32 = _S31.z;

#line 499
        float blocked_1;

#line 499
        if(_S32 >= (at_0.z + _S22))
        {

#line 499
            blocked_1 = blocked_0 + saturate(_S21 / max(abs(centre_1.z - _S32), 0.00000999999974738f));

#line 499
        }
        else
        {

#line 499
            blocked_1 = blocked_0;

#line 499
        }

#line 499
        blocked_0 = blocked_1;

#line 466
        index_0 = index_0 + 1U;

#line 466
    }

#line 508
    return blocked_0 / 8.0f;
}


#line 508
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 508
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 541
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S33 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 541
    thread KernelContext_0 kernelContext_9;

#line 541
    (&kernelContext_9)->scene_depth_0 = scene_depth_1;

#line 541
    (&kernelContext_9)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 551
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_6 = int2(int(width_0), int(height_0));
    float2 size_2 = float2(float(width_0), float(height_0));

#line 561
    int2 _S34 = int2(position_0.xy);
    int2 pixel_7 = full_res_pixel_0(_S34);
    uint tile_1 = (uint(_S34.y) & 3U) * 4U + (uint(_S34.x) & 3U);

#line 563
    float _S35 = depth_at_0(pixel_7, extent_6, &kernelContext_9);



    if(_S35 <= 0.0f)
    {

#line 567
        pixelOutput_0 _S36 = { float4(1.0f, encode_bent_0(float3(0.0f, 0.0f, 0.0f))) };

        return _S36;
    }

#line 569
    float3 _S37 = view_position_0(pixel_7, _S35, size_2, &kernelContext_9);

#line 569
    float3 _S38 = normal_at_0(pixel_7, _S37, extent_6, size_2, &kernelContext_9);

#line 569
    float _S39 = occlusion_at_0(pixel_7, tile_1, _S37, _S38, extent_6, size_2, &kernelContext_9);

#line 569
    pixelOutput_0 _S40 = { float4(saturate(1.0f - _S39), encode_bent_0(float3(0.0f, 0.0f, 0.0f))) };

#line 577
    return _S40;
}


#line 577
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 257
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 257
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 257
    thread KernelContext_0 kernelContext_10;

#line 257
    (&kernelContext_10)->scene_depth_0 = scene_depth_2;

#line 257
    (&kernelContext_10)->camera_0 = camera_2;

#line 514
    thread FullscreenOutput_0 output_1;


    float2 _S41 = float2(float((index_1 << 1U) & 2U), float(index_1 & 2U));

#line 517
    (&output_1)->uv_2 = _S41;
    (&output_1)->position_2 = float4(_S41 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 518
    thread vertexMain_Result_0 _S42;

#line 518
    (&_S42)->position_1 = output_1.position_2;

#line 518
    (&_S42)->uv_1 = output_1.uv_2;

#line 518
    return _S42;
}

