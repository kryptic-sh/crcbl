#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 363 "shaders/ssao.slang"
constant array<float, int(16)> STEP_OFFSETS_0 = { 0.0625f, 0.5625f, 0.1875f, 0.6875f, 0.8125f, 0.3125f, 0.9375f, 0.4375f, 0.25f, 0.75f, 0.125f, 0.625f, 1.0f, 0.5f, 0.875f, 0.375f };

#line 328
constant array<float2, int(16)> SLICE_DIRECTIONS_0 = { float2(2.0f, 0.0f), float2(-2.0f, 0.0f), float2(1.0f, 1.0f), float2(-1.0f, -1.0f), float2(0.0f, -2.0f), float2(0.0f, 2.0f), float2(1.0f, -1.0f), float2(-1.0f, 1.0f), float2(1.0f, 2.0f), float2(-1.0f, -2.0f), float2(2.0f, 1.0f), float2(-2.0f, -1.0f), float2(2.0f, -1.0f), float2(-2.0f, 1.0f), float2(1.0f, -2.0f), float2(-1.0f, 2.0f) };

#line 387
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
    float4 params_0;
};


#line 1084
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    SsaoParams_natural_0 constant* camera_0;
};


#line 398 "shaders/ssao.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 401
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 398
float depth_at_1(int2 pixel_2, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_2, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 401
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 410
float3 view_position_0(int2 pixel_3, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 420
    float4 view_0 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 410
float3 view_position_1(int2 pixel_4, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 420
    float4 view_1 = (((float4(float2((float(pixel_4.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_4.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 436
float3 normal_at_0(int2 pixel_5, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_5 + int2(int(-1), int(0));

#line 438
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 438
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_5 + int2(int(1), int(0));

#line 439
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 439
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_5 + int2(int(0), int(-1));

#line 440
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 440
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_5 + int2(int(0), int(1));

#line 441
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 441
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 443
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 444
        horizontal_0 = _S8 - centre_0;

#line 444
    }
    else
    {

#line 444
        horizontal_0 = centre_0 - _S5;

#line 444
    }

#line 444
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 447
        vertical_0 = _S14 - centre_0;

#line 447
    }
    else
    {

#line 447
        vertical_0 = centre_0 - _S11;

#line 447
    }

#line 457
    return normalize(cross(vertical_0, horizontal_0));
}


#line 599
uint slice_count_0(KernelContext_0 thread* kernelContext_5)
{
    return clamp(uint(kernelContext_5->camera_0->params_0.y), 2U, 4U);
}


#line 615
float2 turned_0(float2 seed_0, uint slice_0)
{

#line 615
    float2 eighth_0;


    if((slice_0 & 2U) != 0U)
    {

#line 618
        float _S16 = seed_0.x;

#line 618
        float _S17 = seed_0.y;

#line 618
        eighth_0 = float2(_S16 - _S17, _S16 + _S17);

#line 618
    }
    else
    {

#line 618
        eighth_0 = seed_0;

#line 618
    }

    if((slice_0 & 1U) != 0U)
    {

#line 620
        eighth_0 = float2(- eighth_0.y, eighth_0.x);

#line 620
    }

#line 620
    return eighth_0;
}


#line 472
float acos_approx_0(float x_0)
{
    float _S18 = min(abs(x_0), 1.0f);

#line 479
    float positive_0 = (((-0.01872929930686951f * _S18 + 0.07426100224256516f) * _S18 + -0.21211439371109009f) * _S18 + 1.57072877883911133f) * sqrt(1.0f - _S18);

#line 479
    float _S19;
    if(x_0 < 0.0f)
    {

#line 480
        _S19 = 3.14159274101257324f - positive_0;

#line 480
    }
    else
    {

#line 480
        _S19 = positive_0;

#line 480
    }

#line 480
    return _S19;
}


#line 555
float horizon_cosine_0(int2 pixel_6, float2 step_0, float offset_0, float reach_0, float3 centre_1, float3 view_2, float radius_0, int2 extent_5, float2 size_1, KernelContext_0 thread* kernelContext_6)
{

#line 556
    float cosine_0 = -1.0f;

#line 556
    uint index_0 = 0U;


    for(;;)
    {

#line 559
        if(index_0 < 4U)
        {
        }
        else
        {

#line 559
            break;
        }

#line 565
        int2 tap_0 = pixel_6 + int2(step_0 * float2((reach_0 * (float(index_0) + offset_0) / 4.0f)) );
        int _S20 = tap_0.x;

#line 566
        bool _S21;

#line 566
        if(_S20 < int(0))
        {

#line 566
            _S21 = true;

#line 566
        }
        else
        {

#line 566
            _S21 = (tap_0.y) < int(0);

#line 566
        }

#line 566
        bool _S22;

#line 566
        if(_S21)
        {

#line 566
            _S22 = true;

#line 566
        }
        else
        {

#line 566
            _S22 = _S20 >= (extent_5.x);

#line 566
        }

#line 566
        bool _S23;

#line 566
        if(_S22)
        {

#line 566
            _S23 = true;

#line 566
        }
        else
        {

#line 566
            _S23 = (tap_0.y) >= (extent_5.y);

#line 566
        }

#line 566
        if(_S23)
        {
            break;
        }

#line 568
        float _S24 = depth_at_1(tap_0, extent_5, kernelContext_6);



        if(_S24 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 559
            continue;
        }

#line 559
        float3 _S25 = view_position_1(tap_0, _S24, size_1, kernelContext_6);

#line 576
        float3 delta_0 = _S25 - centre_1;
        float length_squared_0 = dot(delta_0, delta_0);

#line 577
        bool _S26;
        if(length_squared_0 > (radius_0 * radius_0))
        {

#line 578
            _S26 = true;

#line 578
        }
        else
        {

#line 578
            _S26 = length_squared_0 < 1.00000001335143196e-10f;

#line 578
        }

#line 578
        if(_S26)
        {
            index_0 = index_0 + 1U;

#line 559
            continue;
        }

#line 559
        cosine_0 = max(cosine_0, dot(delta_0, view_2) / sqrt(length_squared_0));

#line 559
        index_0 = index_0 + 1U;

#line 559
    }

#line 588
    return cosine_0;
}


#line 512
float slice_visibility_0(float h1_0, float cos_h1_0, float sin_h1_0, float h2_0, float cos_h2_0, float sin_h2_0, float cos_gamma_0, float sin_gamma_0)
{

#line 527
    return 0.25f * (- ((2.0f * cos_h1_0 * cos_h1_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h1_0 * cos_h1_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h1_0 * sin_gamma_0 + (- ((2.0f * cos_h2_0 * cos_h2_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h2_0 * cos_h2_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h2_0 * sin_gamma_0));
}


#line 636
float occlusion_at_0(int2 pixel_7, uint tile_0, float3 centre_2, float3 normal_0, int2 extent_6, float2 size_2, KernelContext_0 thread* kernelContext_7)
{
    float radius_1 = kernelContext_7->camera_0->params_0.x;

#line 644
    float4 near_clip_0 = (((float4(centre_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_7->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_7->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_7->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_7->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_7->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_7->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_7->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_7->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_7->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_7->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_7->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_7->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_7->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_7->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_7->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_7->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 far_clip_0 = (((float4(centre_2 + float3(radius_1, 0.0f, 0.0f), 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_7->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_7->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_7->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_7->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_7->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_7->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_7->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_7->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_7->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_7->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_7->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_7->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_7->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_7->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_7->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_7->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S27 = near_clip_0.w;

#line 646
    bool _S28;

#line 646
    if(_S27 <= 0.0f)
    {

#line 646
        _S28 = true;

#line 646
    }
    else
    {

#line 646
        _S28 = (far_clip_0.w) <= 0.0f;

#line 646
    }

#line 646
    if(_S28)
    {
        return 0.0f;
    }
    float reach_1 = abs(far_clip_0.x / far_clip_0.w - near_clip_0.x / _S27) * 0.5f * size_2.x;
    if(reach_1 < 2.0f)
    {


        return 0.0f;
    }



    float3 _S29 = normalize(- centre_2);

#line 660
    uint _S30 = slice_count_0(kernelContext_7);

#line 660
    uint slice_1 = 0U;

#line 660
    float visibility_0 = 0.0f;

#line 660
    float weight_0 = 0.0f;

#line 672
    for(;;)
    {

#line 672
        if(slice_1 < 4U)
        {
        }
        else
        {

#line 672
            break;
        }
        if(slice_1 >= _S30)
        {
            break;
        }

#line 682
        float2 direction_0 = normalize(turned_0(SLICE_DIRECTIONS_0[tile_0], slice_1));

#line 690
        float3 axis_0 = normalize(cross(float3(direction_0.x, - direction_0.y, 0.0f), _S29));
        float3 projected_0 = normal_0 - axis_0 * float3(dot(normal_0, axis_0)) ;
        float projected_length_0 = length(projected_0);
        if(projected_length_0 < 9.99999997475242708e-07f)
        {



            slice_1 = slice_1 + 1U;

#line 672
            continue;
        }

#line 714
        float cos_gamma_1 = clamp(dot(projected_0, _S29) / projected_length_0, -1.0f, 1.0f);

#line 714
        float sign_gamma_0;
        if((dot(cross(_S29, axis_0), projected_0)) < 0.0f)
        {

#line 715
            sign_gamma_0 = -1.0f;

#line 715
        }
        else
        {

#line 715
            sign_gamma_0 = 1.0f;

#line 715
        }
        float gamma_0 = sign_gamma_0 * acos_approx_0(cos_gamma_1);
        float sin_gamma_1 = sign_gamma_0 * sqrt(saturate(1.0f - cos_gamma_1 * cos_gamma_1));

#line 717
        float _S31 = horizon_cosine_0(pixel_7, - direction_0, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S29, radius_1, extent_6, size_2, kernelContext_7);

#line 717
        float _S32 = horizon_cosine_0(pixel_7, direction_0, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S29, radius_1, extent_6, size_2, kernelContext_7);

#line 733
        float raw_low_0 = - acos_approx_0(_S31);
        float low_0 = gamma_0 - 1.57079637050628662f;
        bool clamped_low_0 = raw_low_0 < low_0;

#line 735
        float h1_1;
        if(clamped_low_0)
        {

#line 736
            h1_1 = low_0;

#line 736
        }
        else
        {

#line 736
            h1_1 = raw_low_0;

#line 736
        }

#line 736
        float cos_h1_1;
        if(clamped_low_0)
        {

#line 737
            cos_h1_1 = sin_gamma_1;

#line 737
        }
        else
        {

#line 737
            cos_h1_1 = _S31;

#line 737
        }

#line 737
        float sin_h1_1;

        if(clamped_low_0)
        {

#line 739
            sin_h1_1 = - cos_gamma_1;

#line 739
        }
        else
        {

#line 739
            sin_h1_1 = - sqrt(saturate(1.0f - _S31 * _S31));

#line 739
        }

        float raw_high_0 = acos_approx_0(_S32);
        float high_0 = gamma_0 + 1.57079637050628662f;
        bool clamped_high_0 = raw_high_0 > high_0;

#line 743
        float h2_1;
        if(clamped_high_0)
        {

#line 744
            h2_1 = high_0;

#line 744
        }
        else
        {

#line 744
            h2_1 = raw_high_0;

#line 744
        }

#line 744
        float cos_h2_1;
        if(clamped_high_0)
        {

#line 745
            cos_h2_1 = - sin_gamma_1;

#line 745
        }
        else
        {

#line 745
            cos_h2_1 = _S32;

#line 745
        }

#line 745
        float sin_h2_1;

        if(clamped_high_0)
        {

#line 747
            sin_h2_1 = cos_gamma_1;

#line 747
        }
        else
        {

#line 747
            sin_h2_1 = sqrt(saturate(1.0f - _S32 * _S32));

#line 747
        }

#line 752
        float weight_1 = weight_0 + projected_length_0;

#line 752
        visibility_0 = visibility_0 + projected_length_0 * slice_visibility_0(h1_1, cos_h1_1, sin_h1_1, h2_1, cos_h2_1, sin_h2_1, cos_gamma_1, sin_gamma_1);

#line 752
        weight_0 = weight_1;

#line 672
        slice_1 = slice_1 + 1U;

#line 672
    }

#line 755
    if(weight_0 <= 0.0f)
    {
        return 0.0f;
    }
    return saturate(1.0f - visibility_0 / weight_0);
}


#line 759
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 759
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 774
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S33 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 774
    thread KernelContext_0 kernelContext_8;

#line 774
    (&kernelContext_8)->scene_depth_0 = scene_depth_1;

#line 774
    (&kernelContext_8)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 784
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_7 = int2(int(width_0), int(height_0));
    float2 size_3 = float2(float(width_0), float(height_0));

#line 794
    int2 _S34 = int2(position_0.xy);
    int2 pixel_8 = full_res_pixel_0(_S34);
    uint tile_1 = (uint(_S34.y) & 3U) * 4U + (uint(_S34.x) & 3U);

#line 796
    float _S35 = depth_at_0(pixel_8, extent_7, &kernelContext_8);



    if(_S35 <= 0.0f)
    {

#line 800
        pixelOutput_0 _S36 = { 1.0f };

        return _S36;
    }

#line 802
    float3 _S37 = view_position_0(pixel_8, _S35, size_3, &kernelContext_8);

#line 802
    float3 _S38 = normal_at_0(pixel_8, _S37, extent_7, size_3, &kernelContext_8);

#line 802
    float _S39 = occlusion_at_0(pixel_8, tile_1, _S37, _S38, extent_7, size_3, &kernelContext_8);

#line 802
    pixelOutput_0 _S40 = { saturate(1.0f - _S39) };

#line 807
    return _S40;
}


#line 807
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 370
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 370
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 370
    thread KernelContext_0 kernelContext_9;

#line 370
    (&kernelContext_9)->scene_depth_0 = scene_depth_2;

#line 370
    (&kernelContext_9)->camera_0 = camera_2;

#line 765
    thread FullscreenOutput_0 output_1;


    float2 _S41 = float2(float((index_1 << 1U) & 2U), float(index_1 & 2U));

#line 768
    (&output_1)->uv_2 = _S41;
    (&output_1)->position_2 = float4(_S41 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 769
    thread vertexMain_Result_0 _S42;

#line 769
    (&_S42)->position_1 = output_1.position_2;

#line 769
    (&_S42)->uv_1 = output_1.uv_2;

#line 769
    return _S42;
}

