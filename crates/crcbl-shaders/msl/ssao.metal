#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 237 "shaders/ssao.slang"
constant array<float, int(16)> STEP_OFFSETS_0 = { 0.0625f, 0.5625f, 0.1875f, 0.6875f, 0.8125f, 0.3125f, 0.9375f, 0.4375f, 0.25f, 0.75f, 0.125f, 0.625f, 1.0f, 0.5f, 0.875f, 0.375f };

#line 202
constant array<float2, int(16)> SLICE_DIRECTIONS_0 = { float2(2.0f, 0.0f), float2(-2.0f, 0.0f), float2(1.0f, 1.0f), float2(-1.0f, -1.0f), float2(0.0f, -2.0f), float2(0.0f, 2.0f), float2(1.0f, -1.0f), float2(-1.0f, 1.0f), float2(1.0f, 2.0f), float2(-1.0f, -2.0f), float2(2.0f, 1.0f), float2(-2.0f, -1.0f), float2(2.0f, -1.0f), float2(-2.0f, 1.0f), float2(1.0f, -2.0f), float2(-1.0f, 2.0f) };

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
    float4 params_0;
};


#line 1084
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    SsaoParams_natural_0 constant* camera_0;
};


#line 256 "shaders/ssao.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 259
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 256
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 259
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 268
float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 278
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 268
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 278
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 294
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 296
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 296
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 297
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 297
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 298
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 298
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 299
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 299
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 301
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 302
        horizontal_0 = _S8 - centre_0;

#line 302
    }
    else
    {

#line 302
        horizontal_0 = centre_0 - _S5;

#line 302
    }

#line 302
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 305
        vertical_0 = _S14 - centre_0;

#line 305
    }
    else
    {

#line 305
        vertical_0 = centre_0 - _S11;

#line 305
    }

#line 315
    return normalize(cross(vertical_0, horizontal_0));
}


#line 330
float acos_approx_0(float x_0)
{
    float _S16 = min(abs(x_0), 1.0f);

#line 337
    float positive_0 = (((-0.01872929930686951f * _S16 + 0.07426100224256516f) * _S16 + -0.21211439371109009f) * _S16 + 1.57072877883911133f) * sqrt(1.0f - _S16);

#line 337
    float _S17;
    if(x_0 < 0.0f)
    {

#line 338
        _S17 = 3.14159274101257324f - positive_0;

#line 338
    }
    else
    {

#line 338
        _S17 = positive_0;

#line 338
    }

#line 338
    return _S17;
}


#line 413
float horizon_cosine_0(int2 pixel_5, float2 step_0, float offset_0, float reach_0, float3 centre_1, float3 view_2, float radius_0, int2 extent_5, float2 size_1, KernelContext_0 thread* kernelContext_5)
{

#line 414
    float cosine_0 = -1.0f;

#line 414
    uint index_0 = 0U;


    for(;;)
    {

#line 417
        if(index_0 < 4U)
        {
        }
        else
        {

#line 417
            break;
        }

#line 423
        int2 tap_0 = pixel_5 + int2(step_0 * float2((reach_0 * (float(index_0) + offset_0) / 4.0f)) );
        int _S18 = tap_0.x;

#line 424
        bool _S19;

#line 424
        if(_S18 < int(0))
        {

#line 424
            _S19 = true;

#line 424
        }
        else
        {

#line 424
            _S19 = (tap_0.y) < int(0);

#line 424
        }

#line 424
        bool _S20;

#line 424
        if(_S19)
        {

#line 424
            _S20 = true;

#line 424
        }
        else
        {

#line 424
            _S20 = _S18 >= (extent_5.x);

#line 424
        }

#line 424
        bool _S21;

#line 424
        if(_S20)
        {

#line 424
            _S21 = true;

#line 424
        }
        else
        {

#line 424
            _S21 = (tap_0.y) >= (extent_5.y);

#line 424
        }

#line 424
        if(_S21)
        {
            break;
        }

#line 426
        float _S22 = depth_at_1(tap_0, extent_5, kernelContext_5);



        if(_S22 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 417
            continue;
        }

#line 417
        float3 _S23 = view_position_1(tap_0, _S22, size_1, kernelContext_5);

#line 434
        float3 delta_0 = _S23 - centre_1;
        float length_squared_0 = dot(delta_0, delta_0);

#line 435
        bool _S24;
        if(length_squared_0 > (radius_0 * radius_0))
        {

#line 436
            _S24 = true;

#line 436
        }
        else
        {

#line 436
            _S24 = length_squared_0 < 1.00000001335143196e-10f;

#line 436
        }

#line 436
        if(_S24)
        {
            index_0 = index_0 + 1U;

#line 417
            continue;
        }

#line 417
        cosine_0 = max(cosine_0, dot(delta_0, view_2) / sqrt(length_squared_0));

#line 417
        index_0 = index_0 + 1U;

#line 417
    }

#line 446
    return cosine_0;
}


#line 370
float slice_visibility_0(float h1_0, float cos_h1_0, float sin_h1_0, float h2_0, float cos_h2_0, float sin_h2_0, float cos_gamma_0, float sin_gamma_0)
{

#line 385
    return 0.25f * (- ((2.0f * cos_h1_0 * cos_h1_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h1_0 * cos_h1_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h1_0 * sin_gamma_0 + (- ((2.0f * cos_h2_0 * cos_h2_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h2_0 * cos_h2_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h2_0 * sin_gamma_0));
}


#line 453
float occlusion_at_0(int2 pixel_6, float3 centre_2, float3 normal_0, int2 extent_6, float2 size_2, KernelContext_0 thread* kernelContext_6)
{
    float radius_1 = kernelContext_6->camera_0->params_0.x;

#line 461
    float4 near_clip_0 = (((float4(centre_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_6->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 far_clip_0 = (((float4(centre_2 + float3(radius_1, 0.0f, 0.0f), 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_6->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S25 = near_clip_0.w;

#line 463
    bool _S26;

#line 463
    if(_S25 <= 0.0f)
    {

#line 463
        _S26 = true;

#line 463
    }
    else
    {

#line 463
        _S26 = (far_clip_0.w) <= 0.0f;

#line 463
    }

#line 463
    if(_S26)
    {
        return 0.0f;
    }
    float reach_1 = abs(far_clip_0.x / far_clip_0.w - near_clip_0.x / _S25) * 0.5f * size_2.x;
    if(reach_1 < 2.0f)
    {


        return 0.0f;
    }



    float3 _S27 = normalize(- centre_2);

    uint tile_0 = (uint(pixel_6.y) & 3U) * 4U + (uint(pixel_6.x) & 3U);

#line 479
    float2 direction_0 = normalize(SLICE_DIRECTIONS_0[tile_0]);

#line 479
    uint slice_0 = 0U;

#line 479
    float visibility_0 = 0.0f;

#line 479
    float weight_0 = 0.0f;

#line 485
    for(;;)
    {

#line 485
        if(slice_0 < 2U)
        {
        }
        else
        {

#line 485
            break;
        }



        float _S28 = direction_0.x;

#line 490
        float _S29 = - direction_0.y;



        float3 axis_0 = normalize(cross(float3(_S28, _S29, 0.0f), _S27));
        float3 projected_0 = normal_0 - axis_0 * float3(dot(normal_0, axis_0)) ;
        float projected_length_0 = length(projected_0);
        if(projected_length_0 < 9.99999997475242708e-07f)
        {

#line 497
            direction_0 = float2(_S29, _S28);

#line 503
            slice_0 = slice_0 + 1U;

#line 485
            continue;
        }

#line 519
        float cos_gamma_1 = clamp(dot(projected_0, _S27) / projected_length_0, -1.0f, 1.0f);

#line 519
        float sign_gamma_0;
        if((dot(cross(_S27, axis_0), projected_0)) < 0.0f)
        {

#line 520
            sign_gamma_0 = -1.0f;

#line 520
        }
        else
        {

#line 520
            sign_gamma_0 = 1.0f;

#line 520
        }
        float gamma_0 = sign_gamma_0 * acos_approx_0(cos_gamma_1);
        float sin_gamma_1 = sign_gamma_0 * sqrt(saturate(1.0f - cos_gamma_1 * cos_gamma_1));

#line 522
        float _S30 = horizon_cosine_0(pixel_6, - direction_0, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S27, radius_1, extent_6, size_2, kernelContext_6);

#line 522
        float _S31 = horizon_cosine_0(pixel_6, direction_0, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S27, radius_1, extent_6, size_2, kernelContext_6);

#line 538
        float raw_low_0 = - acos_approx_0(_S30);
        float low_0 = gamma_0 - 1.57079637050628662f;
        bool clamped_low_0 = raw_low_0 < low_0;

#line 540
        float h1_1;
        if(clamped_low_0)
        {

#line 541
            h1_1 = low_0;

#line 541
        }
        else
        {

#line 541
            h1_1 = raw_low_0;

#line 541
        }

#line 541
        float cos_h1_1;
        if(clamped_low_0)
        {

#line 542
            cos_h1_1 = sin_gamma_1;

#line 542
        }
        else
        {

#line 542
            cos_h1_1 = _S30;

#line 542
        }

#line 542
        float sin_h1_1;

        if(clamped_low_0)
        {

#line 544
            sin_h1_1 = - cos_gamma_1;

#line 544
        }
        else
        {

#line 544
            sin_h1_1 = - sqrt(saturate(1.0f - _S30 * _S30));

#line 544
        }

        float raw_high_0 = acos_approx_0(_S31);
        float high_0 = gamma_0 + 1.57079637050628662f;
        bool clamped_high_0 = raw_high_0 > high_0;

#line 548
        float h2_1;
        if(clamped_high_0)
        {

#line 549
            h2_1 = high_0;

#line 549
        }
        else
        {

#line 549
            h2_1 = raw_high_0;

#line 549
        }

#line 549
        float cos_h2_1;
        if(clamped_high_0)
        {

#line 550
            cos_h2_1 = - sin_gamma_1;

#line 550
        }
        else
        {

#line 550
            cos_h2_1 = _S31;

#line 550
        }

#line 550
        float sin_h2_1;

        if(clamped_high_0)
        {

#line 552
            sin_h2_1 = cos_gamma_1;

#line 552
        }
        else
        {

#line 552
            sin_h2_1 = sqrt(saturate(1.0f - _S31 * _S31));

#line 552
        }

        float visibility_1 = visibility_0 + projected_length_0 * slice_visibility_0(h1_1, cos_h1_1, sin_h1_1, h2_1, cos_h2_1, sin_h2_1, cos_gamma_1, sin_gamma_1);


        float weight_1 = weight_0 + projected_length_0;

#line 557
        direction_0 = float2(_S29, _S28);

#line 557
        visibility_0 = visibility_1;

#line 557
        weight_0 = weight_1;

#line 485
        slice_0 = slice_0 + 1U;

#line 485
    }

#line 564
    if(weight_0 <= 0.0f)
    {
        return 0.0f;
    }
    return saturate(1.0f - visibility_0 / weight_0);
}


#line 568
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 568
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 583
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S32 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 583
    thread KernelContext_0 kernelContext_7;

#line 583
    (&kernelContext_7)->scene_depth_0 = scene_depth_1;

#line 583
    (&kernelContext_7)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;



    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_7 = int2(int(width_0), int(height_0));
    float2 size_3 = float2(float(width_0), float(height_0));

    int2 _S33 = int2(position_0.xy);

#line 594
    float _S34 = depth_at_0(_S33, extent_7, &kernelContext_7);



    if(_S34 <= 0.0f)
    {

#line 598
        pixelOutput_0 _S35 = { 1.0f };

        return _S35;
    }

#line 600
    float3 _S36 = view_position_0(_S33, _S34, size_3, &kernelContext_7);

#line 600
    float3 _S37 = normal_at_0(_S33, _S36, extent_7, size_3, &kernelContext_7);

#line 600
    float _S38 = occlusion_at_0(_S33, _S36, _S37, extent_7, size_3, &kernelContext_7);

#line 600
    pixelOutput_0 _S39 = { saturate(1.0f - _S38) };

#line 605
    return _S39;
}


#line 605
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 244
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 244
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 244
    thread KernelContext_0 kernelContext_8;

#line 244
    (&kernelContext_8)->scene_depth_0 = scene_depth_2;

#line 244
    (&kernelContext_8)->camera_0 = camera_2;

#line 574
    thread FullscreenOutput_0 output_1;


    float2 _S40 = float2(float((index_1 << 1U) & 2U), float(index_1 & 2U));

#line 577
    (&output_1)->uv_2 = _S40;
    (&output_1)->position_2 = float4(_S40 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 578
    thread vertexMain_Result_0 _S41;

#line 578
    (&_S41)->position_1 = output_1.position_2;

#line 578
    (&_S41)->uv_1 = output_1.uv_2;

#line 578
    return _S41;
}

