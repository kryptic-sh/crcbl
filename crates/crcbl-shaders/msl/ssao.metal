#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 366 "shaders/ssao.slang"
constant array<float, int(16)> STEP_OFFSETS_0 = { 0.0625f, 0.5625f, 0.1875f, 0.6875f, 0.8125f, 0.3125f, 0.9375f, 0.4375f, 0.25f, 0.75f, 0.125f, 0.625f, 1.0f, 0.5f, 0.875f, 0.375f };

#line 331
constant array<float2, int(16)> SLICE_DIRECTIONS_0 = { float2(2.0f, 0.0f), float2(-2.0f, 0.0f), float2(1.0f, 1.0f), float2(-1.0f, -1.0f), float2(0.0f, -2.0f), float2(0.0f, 2.0f), float2(1.0f, -1.0f), float2(-1.0f, 1.0f), float2(1.0f, 2.0f), float2(-1.0f, -2.0f), float2(2.0f, 1.0f), float2(-2.0f, -1.0f), float2(2.0f, -1.0f), float2(-2.0f, 1.0f), float2(1.0f, -2.0f), float2(-1.0f, 2.0f) };

#line 390
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


#line 401 "shaders/ssao.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 404
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 401
float depth_at_1(int2 pixel_2, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_2, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 404
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 422
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 453
float4 unproject_0(float2 ndc_0, float depth_1, KernelContext_0 thread* kernelContext_3)
{

#line 453
    float2 _S3 = unproject_z_0(depth_1, kernelContext_3);


    return float4((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].y, _S3.x, _S3.y);
}


#line 469
float3 view_position_0(int2 pixel_3, float depth_2, float2 extent_2, KernelContext_0 thread* kernelContext_4)
{

#line 469
    float4 _S4 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_2.y * 2.0f), depth_2, kernelContext_4);

#line 480
    return _S4.xyz / float3(_S4.w) ;
}


#line 469
float3 view_position_1(int2 pixel_4, float depth_3, float2 extent_3, KernelContext_0 thread* kernelContext_5)
{

#line 469
    float4 _S5 = unproject_0(float2((float(pixel_4.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_4.y) + 0.5f) / extent_3.y * 2.0f), depth_3, kernelContext_5);

#line 480
    return _S5.xyz / float3(_S5.w) ;
}


#line 495
float3 normal_at_0(int2 pixel_5, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_6)
{
    int2 _S6 = pixel_5 + int2(int(-1), int(0));

#line 497
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_6);

#line 497
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_6);
    int2 _S9 = pixel_5 + int2(int(1), int(0));

#line 498
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_6);

#line 498
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_6);
    int2 _S12 = pixel_5 + int2(int(0), int(-1));

#line 499
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_6);

#line 499
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_6);
    int2 _S15 = pixel_5 + int2(int(0), int(1));

#line 500
    float _S16 = depth_at_1(_S15, extent_4, kernelContext_6);

#line 500
    float3 _S17 = view_position_1(_S15, _S16, size_0, kernelContext_6);

    float _S18 = centre_0.z;

#line 502
    float3 horizontal_0;
    if((abs(_S11.z - _S18)) < (abs(_S18 - _S8.z)))
    {

#line 503
        horizontal_0 = _S11 - centre_0;

#line 503
    }
    else
    {

#line 503
        horizontal_0 = centre_0 - _S8;

#line 503
    }

#line 503
    float3 vertical_0;


    if((abs(_S17.z - _S18)) < (abs(_S18 - _S14.z)))
    {

#line 506
        vertical_0 = _S17 - centre_0;

#line 506
    }
    else
    {

#line 506
        vertical_0 = centre_0 - _S14;

#line 506
    }

#line 516
    return normalize(cross(vertical_0, horizontal_0));
}


#line 658
uint slice_count_0(KernelContext_0 thread* kernelContext_7)
{
    return clamp(uint(kernelContext_7->camera_0->params_0.y), 2U, 4U);
}


#line 674
float2 turned_0(float2 seed_0, uint slice_0)
{

#line 674
    float2 eighth_0;


    if((slice_0 & 2U) != 0U)
    {

#line 677
        float _S19 = seed_0.x;

#line 677
        float _S20 = seed_0.y;

#line 677
        eighth_0 = float2(_S19 - _S20, _S19 + _S20);

#line 677
    }
    else
    {

#line 677
        eighth_0 = seed_0;

#line 677
    }

    if((slice_0 & 1U) != 0U)
    {

#line 679
        eighth_0 = float2(- eighth_0.y, eighth_0.x);

#line 679
    }

#line 679
    return eighth_0;
}


#line 531
float acos_approx_0(float x_0)
{
    float _S21 = min(abs(x_0), 1.0f);

#line 538
    float positive_0 = (((-0.01872929930686951f * _S21 + 0.07426100224256516f) * _S21 + -0.21211439371109009f) * _S21 + 1.57072877883911133f) * sqrt(1.0f - _S21);

#line 538
    float _S22;
    if(x_0 < 0.0f)
    {

#line 539
        _S22 = 3.14159274101257324f - positive_0;

#line 539
    }
    else
    {

#line 539
        _S22 = positive_0;

#line 539
    }

#line 539
    return _S22;
}


#line 614
float horizon_cosine_0(int2 pixel_6, float2 step_0, float offset_0, float reach_0, float3 centre_1, float3 view_0, float radius_0, int2 extent_5, float2 size_1, KernelContext_0 thread* kernelContext_8)
{

#line 615
    float cosine_0 = -1.0f;

#line 615
    uint index_0 = 0U;


    for(;;)
    {

#line 618
        if(index_0 < 4U)
        {
        }
        else
        {

#line 618
            break;
        }

#line 624
        int2 tap_0 = pixel_6 + int2(step_0 * float2((reach_0 * (float(index_0) + offset_0) / 4.0f)) );
        int _S23 = tap_0.x;

#line 625
        bool _S24;

#line 625
        if(_S23 < int(0))
        {

#line 625
            _S24 = true;

#line 625
        }
        else
        {

#line 625
            _S24 = (tap_0.y) < int(0);

#line 625
        }

#line 625
        bool _S25;

#line 625
        if(_S24)
        {

#line 625
            _S25 = true;

#line 625
        }
        else
        {

#line 625
            _S25 = _S23 >= (extent_5.x);

#line 625
        }

#line 625
        bool _S26;

#line 625
        if(_S25)
        {

#line 625
            _S26 = true;

#line 625
        }
        else
        {

#line 625
            _S26 = (tap_0.y) >= (extent_5.y);

#line 625
        }

#line 625
        if(_S26)
        {
            break;
        }

#line 627
        float _S27 = depth_at_1(tap_0, extent_5, kernelContext_8);



        if(_S27 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 618
            continue;
        }

#line 618
        float3 _S28 = view_position_1(tap_0, _S27, size_1, kernelContext_8);

#line 635
        float3 delta_0 = _S28 - centre_1;
        float length_squared_0 = dot(delta_0, delta_0);

#line 636
        bool _S29;
        if(length_squared_0 > (radius_0 * radius_0))
        {

#line 637
            _S29 = true;

#line 637
        }
        else
        {

#line 637
            _S29 = length_squared_0 < 1.00000001335143196e-10f;

#line 637
        }

#line 637
        if(_S29)
        {
            index_0 = index_0 + 1U;

#line 618
            continue;
        }

#line 618
        cosine_0 = max(cosine_0, dot(delta_0, view_0) / sqrt(length_squared_0));

#line 618
        index_0 = index_0 + 1U;

#line 618
    }

#line 647
    return cosine_0;
}


#line 571
float slice_visibility_0(float h1_0, float cos_h1_0, float sin_h1_0, float h2_0, float cos_h2_0, float sin_h2_0, float cos_gamma_0, float sin_gamma_0)
{

#line 586
    return 0.25f * (- ((2.0f * cos_h1_0 * cos_h1_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h1_0 * cos_h1_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h1_0 * sin_gamma_0 + (- ((2.0f * cos_h2_0 * cos_h2_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h2_0 * cos_h2_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h2_0 * sin_gamma_0));
}


#line 695
float occlusion_at_0(int2 pixel_7, uint tile_0, float3 centre_2, float3 normal_0, int2 extent_6, float2 size_2, KernelContext_0 thread* kernelContext_9)
{
    float radius_1 = kernelContext_9->camera_0->params_0.x;

#line 703
    float4 near_clip_0 = (((float4(centre_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_9->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_9->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_9->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_9->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_9->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_9->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_9->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_9->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_9->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_9->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_9->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_9->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_9->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_9->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_9->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_9->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 far_clip_0 = (((float4(centre_2 + float3(radius_1, 0.0f, 0.0f), 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_9->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_9->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_9->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_9->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_9->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_9->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_9->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_9->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_9->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_9->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_9->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_9->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_9->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_9->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_9->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_9->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S30 = near_clip_0.w;

#line 705
    bool _S31;

#line 705
    if(_S30 <= 0.0f)
    {

#line 705
        _S31 = true;

#line 705
    }
    else
    {

#line 705
        _S31 = (far_clip_0.w) <= 0.0f;

#line 705
    }

#line 705
    if(_S31)
    {
        return 0.0f;
    }
    float reach_1 = abs(far_clip_0.x / far_clip_0.w - near_clip_0.x / _S30) * 0.5f * size_2.x;
    if(reach_1 < 2.0f)
    {


        return 0.0f;
    }



    float3 _S32 = normalize(- centre_2);

#line 719
    uint _S33 = slice_count_0(kernelContext_9);

#line 719
    uint slice_1 = 0U;

#line 719
    float visibility_0 = 0.0f;

#line 719
    float weight_0 = 0.0f;

#line 731
    for(;;)
    {

#line 731
        if(slice_1 < 4U)
        {
        }
        else
        {

#line 731
            break;
        }
        if(slice_1 >= _S33)
        {
            break;
        }

#line 741
        float2 direction_0 = normalize(turned_0(SLICE_DIRECTIONS_0[tile_0], slice_1));

#line 749
        float3 axis_0 = normalize(cross(float3(direction_0.x, - direction_0.y, 0.0f), _S32));
        float3 projected_0 = normal_0 - axis_0 * float3(dot(normal_0, axis_0)) ;
        float projected_length_0 = length(projected_0);
        if(projected_length_0 < 9.99999997475242708e-07f)
        {



            slice_1 = slice_1 + 1U;

#line 731
            continue;
        }

#line 773
        float cos_gamma_1 = clamp(dot(projected_0, _S32) / projected_length_0, -1.0f, 1.0f);

#line 773
        float sign_gamma_0;
        if((dot(cross(_S32, axis_0), projected_0)) < 0.0f)
        {

#line 774
            sign_gamma_0 = -1.0f;

#line 774
        }
        else
        {

#line 774
            sign_gamma_0 = 1.0f;

#line 774
        }
        float gamma_0 = sign_gamma_0 * acos_approx_0(cos_gamma_1);
        float sin_gamma_1 = sign_gamma_0 * sqrt(saturate(1.0f - cos_gamma_1 * cos_gamma_1));

#line 776
        float _S34 = horizon_cosine_0(pixel_7, - direction_0, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S32, radius_1, extent_6, size_2, kernelContext_9);

#line 776
        float _S35 = horizon_cosine_0(pixel_7, direction_0, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S32, radius_1, extent_6, size_2, kernelContext_9);

#line 792
        float raw_low_0 = - acos_approx_0(_S34);
        float low_0 = gamma_0 - 1.57079637050628662f;
        bool clamped_low_0 = raw_low_0 < low_0;

#line 794
        float h1_1;
        if(clamped_low_0)
        {

#line 795
            h1_1 = low_0;

#line 795
        }
        else
        {

#line 795
            h1_1 = raw_low_0;

#line 795
        }

#line 795
        float cos_h1_1;
        if(clamped_low_0)
        {

#line 796
            cos_h1_1 = sin_gamma_1;

#line 796
        }
        else
        {

#line 796
            cos_h1_1 = _S34;

#line 796
        }

#line 796
        float sin_h1_1;

        if(clamped_low_0)
        {

#line 798
            sin_h1_1 = - cos_gamma_1;

#line 798
        }
        else
        {

#line 798
            sin_h1_1 = - sqrt(saturate(1.0f - _S34 * _S34));

#line 798
        }

        float raw_high_0 = acos_approx_0(_S35);
        float high_0 = gamma_0 + 1.57079637050628662f;
        bool clamped_high_0 = raw_high_0 > high_0;

#line 802
        float h2_1;
        if(clamped_high_0)
        {

#line 803
            h2_1 = high_0;

#line 803
        }
        else
        {

#line 803
            h2_1 = raw_high_0;

#line 803
        }

#line 803
        float cos_h2_1;
        if(clamped_high_0)
        {

#line 804
            cos_h2_1 = - sin_gamma_1;

#line 804
        }
        else
        {

#line 804
            cos_h2_1 = _S35;

#line 804
        }

#line 804
        float sin_h2_1;

        if(clamped_high_0)
        {

#line 806
            sin_h2_1 = cos_gamma_1;

#line 806
        }
        else
        {

#line 806
            sin_h2_1 = sqrt(saturate(1.0f - _S35 * _S35));

#line 806
        }

#line 811
        float weight_1 = weight_0 + projected_length_0;

#line 811
        visibility_0 = visibility_0 + projected_length_0 * slice_visibility_0(h1_1, cos_h1_1, sin_h1_1, h2_1, cos_h2_1, sin_h2_1, cos_gamma_1, sin_gamma_1);

#line 811
        weight_0 = weight_1;

#line 731
        slice_1 = slice_1 + 1U;

#line 731
    }

#line 814
    if(weight_0 <= 0.0f)
    {
        return 0.0f;
    }
    return saturate(1.0f - visibility_0 / weight_0);
}


#line 818
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 818
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 833
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S36 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 833
    thread KernelContext_0 kernelContext_10;

#line 833
    (&kernelContext_10)->scene_depth_0 = scene_depth_1;

#line 833
    (&kernelContext_10)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 843
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_7 = int2(int(width_0), int(height_0));
    float2 size_3 = float2(float(width_0), float(height_0));

#line 853
    int2 _S37 = int2(position_0.xy);
    int2 pixel_8 = full_res_pixel_0(_S37);
    uint tile_1 = (uint(_S37.y) & 3U) * 4U + (uint(_S37.x) & 3U);

#line 855
    float _S38 = depth_at_0(pixel_8, extent_7, &kernelContext_10);



    if(_S38 <= 0.0f)
    {

#line 859
        pixelOutput_0 _S39 = { 1.0f };

        return _S39;
    }

#line 861
    float3 _S40 = view_position_0(pixel_8, _S38, size_3, &kernelContext_10);

#line 861
    float3 _S41 = normal_at_0(pixel_8, _S40, extent_7, size_3, &kernelContext_10);

#line 861
    float _S42 = occlusion_at_0(pixel_8, tile_1, _S40, _S41, extent_7, size_3, &kernelContext_10);

#line 861
    pixelOutput_0 _S43 = { saturate(1.0f - _S42) };

#line 866
    return _S43;
}


#line 866
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 373
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 373
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 373
    thread KernelContext_0 kernelContext_11;

#line 373
    (&kernelContext_11)->scene_depth_0 = scene_depth_2;

#line 373
    (&kernelContext_11)->camera_0 = camera_2;

#line 824
    thread FullscreenOutput_0 output_1;


    float2 _S44 = float2(float((index_1 << 1U) & 2U), float(index_1 & 2U));

#line 827
    (&output_1)->uv_2 = _S44;
    (&output_1)->position_2 = float4(_S44 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 828
    thread vertexMain_Result_0 _S45;

#line 828
    (&_S45)->position_1 = output_1.position_2;

#line 828
    (&_S45)->uv_1 = output_1.uv_2;

#line 828
    return _S45;
}

