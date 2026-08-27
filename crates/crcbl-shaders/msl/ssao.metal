#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 198 "shaders/ssao.slang"
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


#line 217 "shaders/ssao.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 220
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 217
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 220
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 229
float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 239
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 229
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 239
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 255
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 257
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 257
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 258
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 258
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 259
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 259
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 260
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 260
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 262
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 263
        horizontal_0 = _S8 - centre_0;

#line 263
    }
    else
    {

#line 263
        horizontal_0 = centre_0 - _S5;

#line 263
    }

#line 263
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 266
        vertical_0 = _S14 - centre_0;

#line 266
    }
    else
    {

#line 266
        vertical_0 = centre_0 - _S11;

#line 266
    }

#line 276
    return normalize(cross(vertical_0, horizontal_0));
}


#line 291
float acos_approx_0(float x_0)
{
    float _S16 = min(abs(x_0), 1.0f);

#line 298
    float positive_0 = (((-0.01872929930686951f * _S16 + 0.07426100224256516f) * _S16 + -0.21211439371109009f) * _S16 + 1.57072877883911133f) * sqrt(1.0f - _S16);

#line 298
    float _S17;
    if(x_0 < 0.0f)
    {

#line 299
        _S17 = 3.14159274101257324f - positive_0;

#line 299
    }
    else
    {

#line 299
        _S17 = positive_0;

#line 299
    }

#line 299
    return _S17;
}


#line 370
float horizon_cosine_0(int2 pixel_5, float2 step_0, float reach_0, float3 centre_1, float3 view_2, float radius_0, int2 extent_5, float2 size_1, KernelContext_0 thread* kernelContext_5)
{

#line 371
    float cosine_0 = -1.0f;

#line 371
    uint index_0 = 0U;


    for(;;)
    {

#line 374
        if(index_0 < 4U)
        {
        }
        else
        {

#line 374
            break;
        }


        uint _S18 = index_0 + 1U;
        int2 tap_0 = pixel_5 + int2(step_0 * float2((reach_0 * float(_S18) / 4.0f)) );
        int _S19 = tap_0.x;

#line 380
        bool _S20;

#line 380
        if(_S19 < int(0))
        {

#line 380
            _S20 = true;

#line 380
        }
        else
        {

#line 380
            _S20 = (tap_0.y) < int(0);

#line 380
        }

#line 380
        bool _S21;

#line 380
        if(_S20)
        {

#line 380
            _S21 = true;

#line 380
        }
        else
        {

#line 380
            _S21 = _S19 >= (extent_5.x);

#line 380
        }

#line 380
        bool _S22;

#line 380
        if(_S21)
        {

#line 380
            _S22 = true;

#line 380
        }
        else
        {

#line 380
            _S22 = (tap_0.y) >= (extent_5.y);

#line 380
        }

#line 380
        if(_S22)
        {
            break;
        }

#line 382
        float _S23 = depth_at_1(tap_0, extent_5, kernelContext_5);



        if(_S23 <= 0.0f)
        {
            index_0 = _S18;

#line 374
            continue;
        }

#line 374
        float3 _S24 = view_position_1(tap_0, _S23, size_1, kernelContext_5);

#line 390
        float3 delta_0 = _S24 - centre_1;
        float length_squared_0 = dot(delta_0, delta_0);

#line 391
        bool _S25;
        if(length_squared_0 > (radius_0 * radius_0))
        {

#line 392
            _S25 = true;

#line 392
        }
        else
        {

#line 392
            _S25 = length_squared_0 < 1.00000001335143196e-10f;

#line 392
        }

#line 392
        if(_S25)
        {
            index_0 = _S18;

#line 374
            continue;
        }

#line 374
        cosine_0 = max(cosine_0, dot(delta_0, view_2) / sqrt(length_squared_0));

#line 374
        index_0 = _S18;

#line 374
    }

#line 402
    return cosine_0;
}


#line 331
float slice_visibility_0(float h1_0, float cos_h1_0, float sin_h1_0, float h2_0, float cos_h2_0, float sin_h2_0, float cos_gamma_0, float sin_gamma_0)
{

#line 346
    return 0.25f * (- ((2.0f * cos_h1_0 * cos_h1_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h1_0 * cos_h1_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h1_0 * sin_gamma_0 + (- ((2.0f * cos_h2_0 * cos_h2_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h2_0 * cos_h2_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h2_0 * sin_gamma_0));
}


#line 409
float occlusion_at_0(int2 pixel_6, float3 centre_2, float3 normal_0, int2 extent_6, float2 size_2, KernelContext_0 thread* kernelContext_6)
{
    float radius_1 = kernelContext_6->camera_0->params_0.x;

#line 417
    float4 near_clip_0 = (((float4(centre_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_6->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 far_clip_0 = (((float4(centre_2 + float3(radius_1, 0.0f, 0.0f), 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_6->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_6->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_6->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S26 = near_clip_0.w;

#line 419
    bool _S27;

#line 419
    if(_S26 <= 0.0f)
    {

#line 419
        _S27 = true;

#line 419
    }
    else
    {

#line 419
        _S27 = (far_clip_0.w) <= 0.0f;

#line 419
    }

#line 419
    if(_S27)
    {
        return 0.0f;
    }
    float reach_1 = abs(far_clip_0.x / far_clip_0.w - near_clip_0.x / _S26) * 0.5f * size_2.x;
    if(reach_1 < 2.0f)
    {


        return 0.0f;
    }



    float3 _S28 = normalize(- centre_2);

#line 433
    float2 direction_0 = normalize(SLICE_DIRECTIONS_0[(uint(pixel_6.y) & 3U) * 4U + (uint(pixel_6.x) & 3U)]);

#line 433
    uint slice_0 = 0U;

#line 433
    float visibility_0 = 0.0f;

#line 433
    float weight_0 = 0.0f;

#line 440
    for(;;)
    {

#line 440
        if(slice_0 < 2U)
        {
        }
        else
        {

#line 440
            break;
        }



        float _S29 = direction_0.x;

#line 445
        float _S30 = - direction_0.y;



        float3 axis_0 = normalize(cross(float3(_S29, _S30, 0.0f), _S28));
        float3 projected_0 = normal_0 - axis_0 * float3(dot(normal_0, axis_0)) ;
        float projected_length_0 = length(projected_0);
        if(projected_length_0 < 9.99999997475242708e-07f)
        {

#line 452
            direction_0 = float2(_S30, _S29);

#line 458
            slice_0 = slice_0 + 1U;

#line 440
            continue;
        }

#line 474
        float cos_gamma_1 = clamp(dot(projected_0, _S28) / projected_length_0, -1.0f, 1.0f);

#line 474
        float sign_gamma_0;
        if((dot(cross(_S28, axis_0), projected_0)) < 0.0f)
        {

#line 475
            sign_gamma_0 = -1.0f;

#line 475
        }
        else
        {

#line 475
            sign_gamma_0 = 1.0f;

#line 475
        }
        float gamma_0 = sign_gamma_0 * acos_approx_0(cos_gamma_1);
        float sin_gamma_1 = sign_gamma_0 * sqrt(saturate(1.0f - cos_gamma_1 * cos_gamma_1));

#line 477
        float _S31 = horizon_cosine_0(pixel_6, - direction_0, reach_1, centre_2, _S28, radius_1, extent_6, size_2, kernelContext_6);

#line 477
        float _S32 = horizon_cosine_0(pixel_6, direction_0, reach_1, centre_2, _S28, radius_1, extent_6, size_2, kernelContext_6);

#line 493
        float raw_low_0 = - acos_approx_0(_S31);
        float low_0 = gamma_0 - 1.57079637050628662f;
        bool clamped_low_0 = raw_low_0 < low_0;

#line 495
        float h1_1;
        if(clamped_low_0)
        {

#line 496
            h1_1 = low_0;

#line 496
        }
        else
        {

#line 496
            h1_1 = raw_low_0;

#line 496
        }

#line 496
        float cos_h1_1;
        if(clamped_low_0)
        {

#line 497
            cos_h1_1 = sin_gamma_1;

#line 497
        }
        else
        {

#line 497
            cos_h1_1 = _S31;

#line 497
        }

#line 497
        float sin_h1_1;

        if(clamped_low_0)
        {

#line 499
            sin_h1_1 = - cos_gamma_1;

#line 499
        }
        else
        {

#line 499
            sin_h1_1 = - sqrt(saturate(1.0f - _S31 * _S31));

#line 499
        }

        float raw_high_0 = acos_approx_0(_S32);
        float high_0 = gamma_0 + 1.57079637050628662f;
        bool clamped_high_0 = raw_high_0 > high_0;

#line 503
        float h2_1;
        if(clamped_high_0)
        {

#line 504
            h2_1 = high_0;

#line 504
        }
        else
        {

#line 504
            h2_1 = raw_high_0;

#line 504
        }

#line 504
        float cos_h2_1;
        if(clamped_high_0)
        {

#line 505
            cos_h2_1 = - sin_gamma_1;

#line 505
        }
        else
        {

#line 505
            cos_h2_1 = _S32;

#line 505
        }

#line 505
        float sin_h2_1;

        if(clamped_high_0)
        {

#line 507
            sin_h2_1 = cos_gamma_1;

#line 507
        }
        else
        {

#line 507
            sin_h2_1 = sqrt(saturate(1.0f - _S32 * _S32));

#line 507
        }

        float visibility_1 = visibility_0 + projected_length_0 * slice_visibility_0(h1_1, cos_h1_1, sin_h1_1, h2_1, cos_h2_1, sin_h2_1, cos_gamma_1, sin_gamma_1);


        float weight_1 = weight_0 + projected_length_0;

#line 512
        direction_0 = float2(_S30, _S29);

#line 512
        visibility_0 = visibility_1;

#line 512
        weight_0 = weight_1;

#line 440
        slice_0 = slice_0 + 1U;

#line 440
    }

#line 519
    if(weight_0 <= 0.0f)
    {
        return 0.0f;
    }
    return saturate(1.0f - visibility_0 / weight_0);
}


#line 523
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 523
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 538
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S33 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 538
    thread KernelContext_0 kernelContext_7;

#line 538
    (&kernelContext_7)->scene_depth_0 = scene_depth_1;

#line 538
    (&kernelContext_7)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;



    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_7 = int2(int(width_0), int(height_0));
    float2 size_3 = float2(float(width_0), float(height_0));

    int2 _S34 = int2(position_0.xy);

#line 549
    float _S35 = depth_at_0(_S34, extent_7, &kernelContext_7);



    if(_S35 <= 0.0f)
    {

#line 553
        pixelOutput_0 _S36 = { 1.0f };

        return _S36;
    }

#line 555
    float3 _S37 = view_position_0(_S34, _S35, size_3, &kernelContext_7);

#line 555
    float3 _S38 = normal_at_0(_S34, _S37, extent_7, size_3, &kernelContext_7);

#line 555
    float _S39 = occlusion_at_0(_S34, _S37, _S38, extent_7, size_3, &kernelContext_7);

#line 555
    pixelOutput_0 _S40 = { saturate(1.0f - _S39) };

#line 560
    return _S40;
}


#line 560
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 205
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 205
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 205
    thread KernelContext_0 kernelContext_8;

#line 205
    (&kernelContext_8)->scene_depth_0 = scene_depth_2;

#line 205
    (&kernelContext_8)->camera_0 = camera_2;

#line 529
    thread FullscreenOutput_0 output_1;


    float2 _S41 = float2(float((index_1 << 1U) & 2U), float(index_1 & 2U));

#line 532
    (&output_1)->uv_2 = _S41;
    (&output_1)->position_2 = float4(_S41 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 533
    thread vertexMain_Result_0 _S42;

#line 533
    (&_S42)->position_1 = output_1.position_2;

#line 533
    (&_S42)->uv_1 = output_1.uv_2;

#line 533
    return _S42;
}

