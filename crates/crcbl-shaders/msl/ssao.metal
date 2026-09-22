#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 487 "shaders/ssao.slang"
constant array<float, int(16)> STEP_OFFSETS_0 = { 0.0625f, 0.5625f, 0.1875f, 0.6875f, 0.8125f, 0.3125f, 0.9375f, 0.4375f, 0.25f, 0.75f, 0.125f, 0.625f, 1.0f, 0.5f, 0.875f, 0.375f };

#line 452
constant array<float2, int(16)> SLICE_DIRECTIONS_0 = { float2(2.0f, 0.0f), float2(-2.0f, 0.0f), float2(1.0f, 1.0f), float2(-1.0f, -1.0f), float2(0.0f, -2.0f), float2(0.0f, 2.0f), float2(1.0f, -1.0f), float2(-1.0f, 1.0f), float2(1.0f, 2.0f), float2(-1.0f, -2.0f), float2(2.0f, 1.0f), float2(-2.0f, -1.0f), float2(2.0f, -1.0f), float2(-2.0f, 1.0f), float2(1.0f, -2.0f), float2(-1.0f, 2.0f) };

#line 511
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


#line 522 "shaders/ssao.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 525
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 522
float depth_at_1(int2 pixel_2, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_2, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 525
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 1108
float3 encode_bent_0(float3 direction_0)
{

#line 1108
    float3 _S3 = float3(0.5f) ;

    return direction_0 * _S3 + _S3;
}


#line 543
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 574
float4 unproject_0(float2 ndc_0, float depth_1, KernelContext_0 thread* kernelContext_3)
{

#line 574
    float2 _S4 = unproject_z_0(depth_1, kernelContext_3);


    return float4((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].y, _S4.x, _S4.y);
}


#line 590
float3 view_position_0(int2 pixel_3, float depth_2, float2 extent_2, KernelContext_0 thread* kernelContext_4)
{

#line 590
    float4 _S5 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_2.y * 2.0f), depth_2, kernelContext_4);

#line 601
    return _S5.xyz / float3(_S5.w) ;
}


#line 590
float3 view_position_1(int2 pixel_4, float depth_3, float2 extent_3, KernelContext_0 thread* kernelContext_5)
{

#line 590
    float4 _S6 = unproject_0(float2((float(pixel_4.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_4.y) + 0.5f) / extent_3.y * 2.0f), depth_3, kernelContext_5);

#line 601
    return _S6.xyz / float3(_S6.w) ;
}


#line 616
float3 normal_at_0(int2 pixel_5, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_6)
{
    int2 _S7 = pixel_5 + int2(int(-1), int(0));

#line 618
    float _S8 = depth_at_1(_S7, extent_4, kernelContext_6);

#line 618
    float3 _S9 = view_position_1(_S7, _S8, size_0, kernelContext_6);
    int2 _S10 = pixel_5 + int2(int(1), int(0));

#line 619
    float _S11 = depth_at_1(_S10, extent_4, kernelContext_6);

#line 619
    float3 _S12 = view_position_1(_S10, _S11, size_0, kernelContext_6);
    int2 _S13 = pixel_5 + int2(int(0), int(-1));

#line 620
    float _S14 = depth_at_1(_S13, extent_4, kernelContext_6);

#line 620
    float3 _S15 = view_position_1(_S13, _S14, size_0, kernelContext_6);
    int2 _S16 = pixel_5 + int2(int(0), int(1));

#line 621
    float _S17 = depth_at_1(_S16, extent_4, kernelContext_6);

#line 621
    float3 _S18 = view_position_1(_S16, _S17, size_0, kernelContext_6);

    float _S19 = centre_0.z;

#line 623
    float3 horizontal_0;
    if((abs(_S12.z - _S19)) < (abs(_S19 - _S9.z)))
    {

#line 624
        horizontal_0 = _S12 - centre_0;

#line 624
    }
    else
    {

#line 624
        horizontal_0 = centre_0 - _S9;

#line 624
    }

#line 624
    float3 vertical_0;


    if((abs(_S18.z - _S19)) < (abs(_S19 - _S15.z)))
    {

#line 627
        vertical_0 = _S18 - centre_0;

#line 627
    }
    else
    {

#line 627
        vertical_0 = centre_0 - _S15;

#line 627
    }

#line 637
    return normalize(cross(vertical_0, horizontal_0));
}


#line 796
float sampling_radius_0(KernelContext_0 thread* kernelContext_7)
{
    float asked_0 = kernelContext_7->camera_0->params_0.x;
    if(asked_0 <= 0.0f)
    {
        return 0.5f;
    }
    return clamp(asked_0, 0.0625f, 4.0f);
}


#line 779
uint slice_count_0(KernelContext_0 thread* kernelContext_8)
{
    return clamp(uint(kernelContext_8->camera_0->params_0.y), 2U, 4U);
}


#line 819
bool bent_normals_0(KernelContext_0 thread* kernelContext_9)
{
    return (kernelContext_9->camera_0->params_0.w) != 0.0f;
}


#line 835
float2 turned_0(float2 seed_0, uint slice_0)
{

#line 835
    float2 eighth_0;


    if((slice_0 & 2U) != 0U)
    {

#line 838
        float _S20 = seed_0.x;

#line 838
        float _S21 = seed_0.y;

#line 838
        eighth_0 = float2(_S20 - _S21, _S20 + _S21);

#line 838
    }
    else
    {

#line 838
        eighth_0 = seed_0;

#line 838
    }

    if((slice_0 & 1U) != 0U)
    {

#line 840
        eighth_0 = float2(- eighth_0.y, eighth_0.x);

#line 840
    }

#line 840
    return eighth_0;
}


#line 652
float acos_approx_0(float x_0)
{
    float _S22 = min(abs(x_0), 1.0f);

#line 659
    float positive_0 = (((-0.01872929930686951f * _S22 + 0.07426100224256516f) * _S22 + -0.21211439371109009f) * _S22 + 1.57072877883911133f) * sqrt(1.0f - _S22);

#line 659
    float _S23;
    if(x_0 < 0.0f)
    {

#line 660
        _S23 = 3.14159274101257324f - positive_0;

#line 660
    }
    else
    {

#line 660
        _S23 = positive_0;

#line 660
    }

#line 660
    return _S23;
}


#line 735
float horizon_cosine_0(int2 pixel_6, float2 step_0, float offset_0, float reach_0, float3 centre_1, float3 view_0, float radius_0, int2 extent_5, float2 size_1, KernelContext_0 thread* kernelContext_10)
{

#line 736
    float cosine_0 = -1.0f;

#line 736
    uint index_0 = 0U;


    for(;;)
    {

#line 739
        if(index_0 < 4U)
        {
        }
        else
        {

#line 739
            break;
        }

#line 745
        int2 tap_0 = pixel_6 + int2(step_0 * float2((reach_0 * (float(index_0) + offset_0) / 4.0f)) );
        int _S24 = tap_0.x;

#line 746
        bool _S25;

#line 746
        if(_S24 < int(0))
        {

#line 746
            _S25 = true;

#line 746
        }
        else
        {

#line 746
            _S25 = (tap_0.y) < int(0);

#line 746
        }

#line 746
        bool _S26;

#line 746
        if(_S25)
        {

#line 746
            _S26 = true;

#line 746
        }
        else
        {

#line 746
            _S26 = _S24 >= (extent_5.x);

#line 746
        }

#line 746
        bool _S27;

#line 746
        if(_S26)
        {

#line 746
            _S27 = true;

#line 746
        }
        else
        {

#line 746
            _S27 = (tap_0.y) >= (extent_5.y);

#line 746
        }

#line 746
        if(_S27)
        {
            break;
        }

#line 748
        float _S28 = depth_at_1(tap_0, extent_5, kernelContext_10);



        if(_S28 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 739
            continue;
        }

#line 739
        float3 _S29 = view_position_1(tap_0, _S28, size_1, kernelContext_10);

#line 756
        float3 delta_0 = _S29 - centre_1;
        float length_squared_0 = dot(delta_0, delta_0);

#line 757
        bool _S30;
        if(length_squared_0 > (radius_0 * radius_0))
        {

#line 758
            _S30 = true;

#line 758
        }
        else
        {

#line 758
            _S30 = length_squared_0 < 1.00000001335143196e-10f;

#line 758
        }

#line 758
        if(_S30)
        {
            index_0 = index_0 + 1U;

#line 739
            continue;
        }

#line 739
        cosine_0 = max(cosine_0, dot(delta_0, view_0) / sqrt(length_squared_0));

#line 739
        index_0 = index_0 + 1U;

#line 739
    }

#line 768
    return cosine_0;
}


#line 692
float slice_visibility_0(float h1_0, float cos_h1_0, float sin_h1_0, float h2_0, float cos_h2_0, float sin_h2_0, float cos_gamma_0, float sin_gamma_0)
{

#line 707
    return 0.25f * (- ((2.0f * cos_h1_0 * cos_h1_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h1_0 * cos_h1_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h1_0 * sin_gamma_0 + (- ((2.0f * cos_h2_0 * cos_h2_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h2_0 * cos_h2_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h2_0 * sin_gamma_0));
}


#line 912
float4 occlusion_at_0(int2 pixel_7, uint tile_0, float3 centre_2, float3 normal_0, int2 extent_6, float2 size_2, KernelContext_0 thread* kernelContext_11)
{



    float4 unoccluded_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 917
    float _S31 = sampling_radius_0(kernelContext_11);

#line 924
    float4 near_clip_0 = (((float4(centre_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_11->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 far_clip_0 = (((float4(centre_2 + float3(_S31, 0.0f, 0.0f), 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_11->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S32 = near_clip_0.w;

#line 926
    bool _S33;

#line 926
    if(_S32 <= 0.0f)
    {

#line 926
        _S33 = true;

#line 926
    }
    else
    {

#line 926
        _S33 = (far_clip_0.w) <= 0.0f;

#line 926
    }

#line 926
    if(_S33)
    {
        return unoccluded_0;
    }
    float reach_1 = abs(far_clip_0.x / far_clip_0.w - near_clip_0.x / _S32) * 0.5f * size_2.x;
    if(reach_1 < 2.0f)
    {


        return unoccluded_0;
    }



    float3 _S34 = normalize(- centre_2);

#line 940
    uint _S35 = slice_count_0(kernelContext_11);

#line 940
    bool _S36 = bent_normals_0(kernelContext_11);

#line 953
    float3 _S37 = float3(0.0f, 0.0f, 0.0f);

#line 953
    uint slice_1 = 0U;

#line 953
    float visibility_0 = 0.0f;

#line 953
    float weight_0 = 0.0f;

#line 953
    float3 bent_0 = _S37;

#line 953
    float bent_weight_0 = 0.0f;

#line 959
    for(;;)
    {

#line 959
        if(slice_1 < 4U)
        {
        }
        else
        {

#line 959
            break;
        }
        if(slice_1 >= _S35)
        {
            break;
        }

#line 969
        float2 direction_1 = normalize(turned_0(SLICE_DIRECTIONS_0[tile_0], slice_1));

#line 977
        float3 axis_0 = normalize(cross(float3(direction_1.x, - direction_1.y, 0.0f), _S34));
        float _S38 = dot(normal_0, axis_0);

#line 978
        float3 projected_0 = normal_0 - axis_0 * float3(_S38) ;
        float projected_length_0 = length(projected_0);
        if(projected_length_0 < 9.99999997475242708e-07f)
        {



            slice_1 = slice_1 + 1U;

#line 959
            continue;
        }

#line 1001
        float cos_gamma_1 = clamp(dot(projected_0, _S34) / projected_length_0, -1.0f, 1.0f);

#line 1001
        float sign_gamma_0;
        if((dot(cross(_S34, axis_0), projected_0)) < 0.0f)
        {

#line 1002
            sign_gamma_0 = -1.0f;

#line 1002
        }
        else
        {

#line 1002
            sign_gamma_0 = 1.0f;

#line 1002
        }
        float gamma_0 = sign_gamma_0 * acos_approx_0(cos_gamma_1);
        float sin_gamma_1 = sign_gamma_0 * sqrt(saturate(1.0f - cos_gamma_1 * cos_gamma_1));

#line 1004
        float _S39 = horizon_cosine_0(pixel_7, - direction_1, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S34, _S31, extent_6, size_2, kernelContext_11);

#line 1004
        float _S40 = horizon_cosine_0(pixel_7, direction_1, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S34, _S31, extent_6, size_2, kernelContext_11);

#line 1020
        float raw_low_0 = - acos_approx_0(_S39);
        float low_0 = gamma_0 - 1.57079637050628662f;
        bool clamped_low_0 = raw_low_0 < low_0;

#line 1022
        float h1_1;
        if(clamped_low_0)
        {

#line 1023
            h1_1 = low_0;

#line 1023
        }
        else
        {

#line 1023
            h1_1 = raw_low_0;

#line 1023
        }

#line 1023
        float cos_h1_1;
        if(clamped_low_0)
        {

#line 1024
            cos_h1_1 = sin_gamma_1;

#line 1024
        }
        else
        {

#line 1024
            cos_h1_1 = _S39;

#line 1024
        }

#line 1024
        float sin_h1_1;

        if(clamped_low_0)
        {

#line 1026
            sin_h1_1 = - cos_gamma_1;

#line 1026
        }
        else
        {

#line 1026
            sin_h1_1 = - sqrt(saturate(1.0f - _S39 * _S39));

#line 1026
        }

        float raw_high_0 = acos_approx_0(_S40);
        float high_0 = gamma_0 + 1.57079637050628662f;
        bool clamped_high_0 = raw_high_0 > high_0;

#line 1030
        float h2_1;
        if(clamped_high_0)
        {

#line 1031
            h2_1 = high_0;

#line 1031
        }
        else
        {

#line 1031
            h2_1 = raw_high_0;

#line 1031
        }

#line 1031
        float cos_h2_1;
        if(clamped_high_0)
        {

#line 1032
            cos_h2_1 = - sin_gamma_1;

#line 1032
        }
        else
        {

#line 1032
            cos_h2_1 = _S40;

#line 1032
        }

#line 1032
        float sin_h2_1;

        if(clamped_high_0)
        {

#line 1034
            sin_h2_1 = cos_gamma_1;

#line 1034
        }
        else
        {

#line 1034
            sin_h2_1 = sqrt(saturate(1.0f - _S40 * _S40));

#line 1034
        }



        float _S41 = projected_length_0 * slice_visibility_0(h1_1, cos_h1_1, sin_h1_1, h2_1, cos_h2_1, sin_h2_1, cos_gamma_1, sin_gamma_1);

#line 1038
        float visibility_1 = visibility_0 + _S41;
        float weight_1 = weight_0 + projected_length_0;

#line 1039
        float bent_weight_1;

#line 1039
        float3 bent_1;

        if(_S36)
        {

#line 1048
            float cos_sum_0 = cos_h1_1 * cos_h2_1 - sin_h1_1 * sin_h2_1;
            float cos_half_0 = sqrt(saturate(0.5f * (1.0f + cos_sum_0)));
            float sin_half_0 = sqrt(saturate(0.5f * (1.0f - cos_sum_0)));

#line 1056
            if((h1_1 + h2_1) < 0.0f)
            {

#line 1056
                bent_weight_1 = - sin_half_0;

#line 1056
            }
            else
            {

#line 1056
                bent_weight_1 = sin_half_0;

#line 1056
            }



            float cos_turn_0 = cos_half_0 * cos_gamma_1 + bent_weight_1 * sin_gamma_1;

#line 1071
            float bent_weight_2 = bent_weight_0 + _S41;

#line 1071
            bent_1 = bent_0 + (normal_0 * float3(cos_turn_0)  - cross(axis_0, normal_0) * float3((bent_weight_1 * cos_gamma_1 - cos_half_0 * sin_gamma_1))  + axis_0 * float3((_S38 * (1.0f - cos_turn_0))) ) * float3(_S41) ;

#line 1071
            bent_weight_1 = bent_weight_2;

#line 1041
        }
        else
        {

#line 1041
            bent_1 = bent_0;

#line 1041
            bent_weight_1 = bent_weight_0;

#line 1041
        }

#line 1041
        visibility_0 = visibility_1;

#line 1041
        weight_0 = weight_1;

#line 1041
        bent_0 = bent_1;

#line 1041
        bent_weight_0 = bent_weight_1;

#line 959
        slice_1 = slice_1 + 1U;

#line 959
    }

#line 1075
    if(weight_0 <= 0.0f)
    {
        return unoccluded_0;
    }
    float occlusion_0 = saturate(1.0f - visibility_0 / weight_0);



    if(bent_weight_0 <= 0.0f)
    {

#line 1083
        _S33 = true;

#line 1083
    }
    else
    {

#line 1083
        _S33 = (length(bent_0 / float3(bent_weight_0) )) < 0.5f;

#line 1083
    }

#line 1083
    if(_S33)
    {
        return float4(occlusion_0, 0.0f, 0.0f, 0.0f);
    }



    return float4(occlusion_0, normalize((((float4(bent_0, 0.0f)) * (matrix<float,int(4),int(4)> (kernelContext_11->camera_0->inv_view_0.data_0[int(0)][int(0)], kernelContext_11->camera_0->inv_view_0.data_0[int(1)][int(0)], kernelContext_11->camera_0->inv_view_0.data_0[int(2)][int(0)], kernelContext_11->camera_0->inv_view_0.data_0[int(3)][int(0)], kernelContext_11->camera_0->inv_view_0.data_0[int(0)][int(1)], kernelContext_11->camera_0->inv_view_0.data_0[int(1)][int(1)], kernelContext_11->camera_0->inv_view_0.data_0[int(2)][int(1)], kernelContext_11->camera_0->inv_view_0.data_0[int(3)][int(1)], kernelContext_11->camera_0->inv_view_0.data_0[int(0)][int(2)], kernelContext_11->camera_0->inv_view_0.data_0[int(1)][int(2)], kernelContext_11->camera_0->inv_view_0.data_0[int(2)][int(2)], kernelContext_11->camera_0->inv_view_0.data_0[int(3)][int(2)], kernelContext_11->camera_0->inv_view_0.data_0[int(0)][int(3)], kernelContext_11->camera_0->inv_view_0.data_0[int(1)][int(3)], kernelContext_11->camera_0->inv_view_0.data_0[int(2)][int(3)], kernelContext_11->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz));
}


#line 1090
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1090
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 1114
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S42 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 1114
    thread KernelContext_0 kernelContext_12;

#line 1114
    (&kernelContext_12)->scene_depth_0 = scene_depth_1;

#line 1114
    (&kernelContext_12)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 1124
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_7 = int2(int(width_0), int(height_0));
    float2 size_3 = float2(float(width_0), float(height_0));

#line 1134
    int2 _S43 = int2(position_0.xy);
    int2 pixel_8 = full_res_pixel_0(_S43);
    uint tile_1 = (uint(_S43.y) & 3U) * 4U + (uint(_S43.x) & 3U);

#line 1136
    float _S44 = depth_at_0(pixel_8, extent_7, &kernelContext_12);



    if(_S44 <= 0.0f)
    {

#line 1140
        pixelOutput_0 _S45 = { float4(1.0f, encode_bent_0(float3(0.0f, 0.0f, 0.0f))) };

        return _S45;
    }

#line 1142
    float3 _S46 = view_position_0(pixel_8, _S44, size_3, &kernelContext_12);

#line 1142
    float3 _S47 = normal_at_0(pixel_8, _S46, extent_7, size_3, &kernelContext_12);

#line 1142
    float4 _S48 = occlusion_at_0(pixel_8, tile_1, _S46, _S47, extent_7, size_3, &kernelContext_12);

#line 1142
    pixelOutput_0 _S49 = { float4(saturate(1.0f - _S48.x), encode_bent_0(_S48.yzw)) };

#line 1148
    return _S49;
}


#line 1148
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 494
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 494
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 494
    thread KernelContext_0 kernelContext_13;

#line 494
    (&kernelContext_13)->scene_depth_0 = scene_depth_2;

#line 494
    (&kernelContext_13)->camera_0 = camera_2;

#line 1096
    thread FullscreenOutput_0 output_1;


    float2 _S50 = float2(float((index_1 << 1U) & 2U), float(index_1 & 2U));

#line 1099
    (&output_1)->uv_2 = _S50;
    (&output_1)->position_2 = float4(_S50 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 1100
    thread vertexMain_Result_0 _S51;

#line 1100
    (&_S51)->position_1 = output_1.position_2;

#line 1100
    (&_S51)->uv_1 = output_1.uv_2;

#line 1100
    return _S51;
}

