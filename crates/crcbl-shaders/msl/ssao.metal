#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 475 "shaders/ssao.slang"
constant array<float, int(16)> STEP_OFFSETS_0 = { 0.0625f, 0.5625f, 0.1875f, 0.6875f, 0.8125f, 0.3125f, 0.9375f, 0.4375f, 0.25f, 0.75f, 0.125f, 0.625f, 1.0f, 0.5f, 0.875f, 0.375f };

#line 440
constant array<float2, int(16)> SLICE_DIRECTIONS_0 = { float2(2.0f, 0.0f), float2(-2.0f, 0.0f), float2(1.0f, 1.0f), float2(-1.0f, -1.0f), float2(0.0f, -2.0f), float2(0.0f, 2.0f), float2(1.0f, -1.0f), float2(-1.0f, 1.0f), float2(1.0f, 2.0f), float2(-1.0f, -2.0f), float2(2.0f, 1.0f), float2(-2.0f, -1.0f), float2(2.0f, -1.0f), float2(-2.0f, 1.0f), float2(1.0f, -2.0f), float2(-1.0f, 2.0f) };

#line 499
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


#line 510 "shaders/ssao.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 513
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 510
float depth_at_1(int2 pixel_2, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_2, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 513
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 1096
float3 encode_bent_0(float3 direction_0)
{

#line 1096
    float3 _S3 = float3(0.5f) ;

    return direction_0 * _S3 + _S3;
}


#line 531
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 562
float4 unproject_0(float2 ndc_0, float depth_1, KernelContext_0 thread* kernelContext_3)
{

#line 562
    float2 _S4 = unproject_z_0(depth_1, kernelContext_3);


    return float4((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].y, _S4.x, _S4.y);
}


#line 578
float3 view_position_0(int2 pixel_3, float depth_2, float2 extent_2, KernelContext_0 thread* kernelContext_4)
{

#line 578
    float4 _S5 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_2.y * 2.0f), depth_2, kernelContext_4);

#line 589
    return _S5.xyz / float3(_S5.w) ;
}


#line 578
float3 view_position_1(int2 pixel_4, float depth_3, float2 extent_3, KernelContext_0 thread* kernelContext_5)
{

#line 578
    float4 _S6 = unproject_0(float2((float(pixel_4.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_4.y) + 0.5f) / extent_3.y * 2.0f), depth_3, kernelContext_5);

#line 589
    return _S6.xyz / float3(_S6.w) ;
}


#line 604
float3 normal_at_0(int2 pixel_5, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_6)
{
    int2 _S7 = pixel_5 + int2(int(-1), int(0));

#line 606
    float _S8 = depth_at_1(_S7, extent_4, kernelContext_6);

#line 606
    float3 _S9 = view_position_1(_S7, _S8, size_0, kernelContext_6);
    int2 _S10 = pixel_5 + int2(int(1), int(0));

#line 607
    float _S11 = depth_at_1(_S10, extent_4, kernelContext_6);

#line 607
    float3 _S12 = view_position_1(_S10, _S11, size_0, kernelContext_6);
    int2 _S13 = pixel_5 + int2(int(0), int(-1));

#line 608
    float _S14 = depth_at_1(_S13, extent_4, kernelContext_6);

#line 608
    float3 _S15 = view_position_1(_S13, _S14, size_0, kernelContext_6);
    int2 _S16 = pixel_5 + int2(int(0), int(1));

#line 609
    float _S17 = depth_at_1(_S16, extent_4, kernelContext_6);

#line 609
    float3 _S18 = view_position_1(_S16, _S17, size_0, kernelContext_6);

    float _S19 = centre_0.z;

#line 611
    float3 horizontal_0;
    if((abs(_S12.z - _S19)) < (abs(_S19 - _S9.z)))
    {

#line 612
        horizontal_0 = _S12 - centre_0;

#line 612
    }
    else
    {

#line 612
        horizontal_0 = centre_0 - _S9;

#line 612
    }

#line 612
    float3 vertical_0;


    if((abs(_S18.z - _S19)) < (abs(_S19 - _S15.z)))
    {

#line 615
        vertical_0 = _S18 - centre_0;

#line 615
    }
    else
    {

#line 615
        vertical_0 = centre_0 - _S15;

#line 615
    }

#line 625
    return normalize(cross(vertical_0, horizontal_0));
}


#line 784
float sampling_radius_0(KernelContext_0 thread* kernelContext_7)
{
    float asked_0 = kernelContext_7->camera_0->params_0.x;
    if(asked_0 <= 0.0f)
    {
        return 0.5f;
    }
    return clamp(asked_0, 0.0625f, 4.0f);
}


#line 767
uint slice_count_0(KernelContext_0 thread* kernelContext_8)
{
    return clamp(uint(kernelContext_8->camera_0->params_0.y), 2U, 4U);
}


#line 807
bool bent_normals_0(KernelContext_0 thread* kernelContext_9)
{
    return (kernelContext_9->camera_0->params_0.w) != 0.0f;
}


#line 823
float2 turned_0(float2 seed_0, uint slice_0)
{

#line 823
    float2 eighth_0;


    if((slice_0 & 2U) != 0U)
    {

#line 826
        float _S20 = seed_0.x;

#line 826
        float _S21 = seed_0.y;

#line 826
        eighth_0 = float2(_S20 - _S21, _S20 + _S21);

#line 826
    }
    else
    {

#line 826
        eighth_0 = seed_0;

#line 826
    }

    if((slice_0 & 1U) != 0U)
    {

#line 828
        eighth_0 = float2(- eighth_0.y, eighth_0.x);

#line 828
    }

#line 828
    return eighth_0;
}


#line 640
float acos_approx_0(float x_0)
{
    float _S22 = min(abs(x_0), 1.0f);

#line 647
    float positive_0 = (((-0.01872929930686951f * _S22 + 0.07426100224256516f) * _S22 + -0.21211439371109009f) * _S22 + 1.57072877883911133f) * sqrt(1.0f - _S22);

#line 647
    float _S23;
    if(x_0 < 0.0f)
    {

#line 648
        _S23 = 3.14159274101257324f - positive_0;

#line 648
    }
    else
    {

#line 648
        _S23 = positive_0;

#line 648
    }

#line 648
    return _S23;
}


#line 723
float horizon_cosine_0(int2 pixel_6, float2 step_0, float offset_0, float reach_0, float3 centre_1, float3 view_0, float radius_0, int2 extent_5, float2 size_1, KernelContext_0 thread* kernelContext_10)
{

#line 724
    float cosine_0 = -1.0f;

#line 724
    uint index_0 = 0U;


    for(;;)
    {

#line 727
        if(index_0 < 4U)
        {
        }
        else
        {

#line 727
            break;
        }

#line 733
        int2 tap_0 = pixel_6 + int2(step_0 * float2((reach_0 * (float(index_0) + offset_0) / 4.0f)) );
        int _S24 = tap_0.x;

#line 734
        bool _S25;

#line 734
        if(_S24 < int(0))
        {

#line 734
            _S25 = true;

#line 734
        }
        else
        {

#line 734
            _S25 = (tap_0.y) < int(0);

#line 734
        }

#line 734
        bool _S26;

#line 734
        if(_S25)
        {

#line 734
            _S26 = true;

#line 734
        }
        else
        {

#line 734
            _S26 = _S24 >= (extent_5.x);

#line 734
        }

#line 734
        bool _S27;

#line 734
        if(_S26)
        {

#line 734
            _S27 = true;

#line 734
        }
        else
        {

#line 734
            _S27 = (tap_0.y) >= (extent_5.y);

#line 734
        }

#line 734
        if(_S27)
        {
            break;
        }

#line 736
        float _S28 = depth_at_1(tap_0, extent_5, kernelContext_10);



        if(_S28 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 727
            continue;
        }

#line 727
        float3 _S29 = view_position_1(tap_0, _S28, size_1, kernelContext_10);

#line 744
        float3 delta_0 = _S29 - centre_1;
        float length_squared_0 = dot(delta_0, delta_0);

#line 745
        bool _S30;
        if(length_squared_0 > (radius_0 * radius_0))
        {

#line 746
            _S30 = true;

#line 746
        }
        else
        {

#line 746
            _S30 = length_squared_0 < 1.00000001335143196e-10f;

#line 746
        }

#line 746
        if(_S30)
        {
            index_0 = index_0 + 1U;

#line 727
            continue;
        }

#line 727
        cosine_0 = max(cosine_0, dot(delta_0, view_0) / sqrt(length_squared_0));

#line 727
        index_0 = index_0 + 1U;

#line 727
    }

#line 756
    return cosine_0;
}


#line 680
float slice_visibility_0(float h1_0, float cos_h1_0, float sin_h1_0, float h2_0, float cos_h2_0, float sin_h2_0, float cos_gamma_0, float sin_gamma_0)
{

#line 695
    return 0.25f * (- ((2.0f * cos_h1_0 * cos_h1_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h1_0 * cos_h1_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h1_0 * sin_gamma_0 + (- ((2.0f * cos_h2_0 * cos_h2_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h2_0 * cos_h2_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h2_0 * sin_gamma_0));
}


#line 900
float4 occlusion_at_0(int2 pixel_7, uint tile_0, float3 centre_2, float3 normal_0, int2 extent_6, float2 size_2, KernelContext_0 thread* kernelContext_11)
{



    float4 unoccluded_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 905
    float _S31 = sampling_radius_0(kernelContext_11);

#line 912
    float4 near_clip_0 = (((float4(centre_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_11->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 far_clip_0 = (((float4(centre_2 + float3(_S31, 0.0f, 0.0f), 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_11->camera_0->proj_0.data_0[int(0)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(0)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(1)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(2)], kernelContext_11->camera_0->proj_0.data_0[int(0)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(1)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(2)][int(3)], kernelContext_11->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S32 = near_clip_0.w;

#line 914
    bool _S33;

#line 914
    if(_S32 <= 0.0f)
    {

#line 914
        _S33 = true;

#line 914
    }
    else
    {

#line 914
        _S33 = (far_clip_0.w) <= 0.0f;

#line 914
    }

#line 914
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

#line 928
    uint _S35 = slice_count_0(kernelContext_11);

#line 928
    bool _S36 = bent_normals_0(kernelContext_11);

#line 941
    float3 _S37 = float3(0.0f, 0.0f, 0.0f);

#line 941
    uint slice_1 = 0U;

#line 941
    float visibility_0 = 0.0f;

#line 941
    float weight_0 = 0.0f;

#line 941
    float3 bent_0 = _S37;

#line 941
    float bent_weight_0 = 0.0f;

#line 947
    for(;;)
    {

#line 947
        if(slice_1 < 4U)
        {
        }
        else
        {

#line 947
            break;
        }
        if(slice_1 >= _S35)
        {
            break;
        }

#line 957
        float2 direction_1 = normalize(turned_0(SLICE_DIRECTIONS_0[tile_0], slice_1));

#line 965
        float3 axis_0 = normalize(cross(float3(direction_1.x, - direction_1.y, 0.0f), _S34));
        float _S38 = dot(normal_0, axis_0);

#line 966
        float3 projected_0 = normal_0 - axis_0 * float3(_S38) ;
        float projected_length_0 = length(projected_0);
        if(projected_length_0 < 9.99999997475242708e-07f)
        {



            slice_1 = slice_1 + 1U;

#line 947
            continue;
        }

#line 989
        float cos_gamma_1 = clamp(dot(projected_0, _S34) / projected_length_0, -1.0f, 1.0f);

#line 989
        float sign_gamma_0;
        if((dot(cross(_S34, axis_0), projected_0)) < 0.0f)
        {

#line 990
            sign_gamma_0 = -1.0f;

#line 990
        }
        else
        {

#line 990
            sign_gamma_0 = 1.0f;

#line 990
        }
        float gamma_0 = sign_gamma_0 * acos_approx_0(cos_gamma_1);
        float sin_gamma_1 = sign_gamma_0 * sqrt(saturate(1.0f - cos_gamma_1 * cos_gamma_1));

#line 992
        float _S39 = horizon_cosine_0(pixel_7, - direction_1, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S34, _S31, extent_6, size_2, kernelContext_11);

#line 992
        float _S40 = horizon_cosine_0(pixel_7, direction_1, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S34, _S31, extent_6, size_2, kernelContext_11);

#line 1008
        float raw_low_0 = - acos_approx_0(_S39);
        float low_0 = gamma_0 - 1.57079637050628662f;
        bool clamped_low_0 = raw_low_0 < low_0;

#line 1010
        float h1_1;
        if(clamped_low_0)
        {

#line 1011
            h1_1 = low_0;

#line 1011
        }
        else
        {

#line 1011
            h1_1 = raw_low_0;

#line 1011
        }

#line 1011
        float cos_h1_1;
        if(clamped_low_0)
        {

#line 1012
            cos_h1_1 = sin_gamma_1;

#line 1012
        }
        else
        {

#line 1012
            cos_h1_1 = _S39;

#line 1012
        }

#line 1012
        float sin_h1_1;

        if(clamped_low_0)
        {

#line 1014
            sin_h1_1 = - cos_gamma_1;

#line 1014
        }
        else
        {

#line 1014
            sin_h1_1 = - sqrt(saturate(1.0f - _S39 * _S39));

#line 1014
        }

        float raw_high_0 = acos_approx_0(_S40);
        float high_0 = gamma_0 + 1.57079637050628662f;
        bool clamped_high_0 = raw_high_0 > high_0;

#line 1018
        float h2_1;
        if(clamped_high_0)
        {

#line 1019
            h2_1 = high_0;

#line 1019
        }
        else
        {

#line 1019
            h2_1 = raw_high_0;

#line 1019
        }

#line 1019
        float cos_h2_1;
        if(clamped_high_0)
        {

#line 1020
            cos_h2_1 = - sin_gamma_1;

#line 1020
        }
        else
        {

#line 1020
            cos_h2_1 = _S40;

#line 1020
        }

#line 1020
        float sin_h2_1;

        if(clamped_high_0)
        {

#line 1022
            sin_h2_1 = cos_gamma_1;

#line 1022
        }
        else
        {

#line 1022
            sin_h2_1 = sqrt(saturate(1.0f - _S40 * _S40));

#line 1022
        }



        float _S41 = projected_length_0 * slice_visibility_0(h1_1, cos_h1_1, sin_h1_1, h2_1, cos_h2_1, sin_h2_1, cos_gamma_1, sin_gamma_1);

#line 1026
        float visibility_1 = visibility_0 + _S41;
        float weight_1 = weight_0 + projected_length_0;

#line 1027
        float bent_weight_1;

#line 1027
        float3 bent_1;

        if(_S36)
        {

#line 1036
            float cos_sum_0 = cos_h1_1 * cos_h2_1 - sin_h1_1 * sin_h2_1;
            float cos_half_0 = sqrt(saturate(0.5f * (1.0f + cos_sum_0)));
            float sin_half_0 = sqrt(saturate(0.5f * (1.0f - cos_sum_0)));

#line 1044
            if((h1_1 + h2_1) < 0.0f)
            {

#line 1044
                bent_weight_1 = - sin_half_0;

#line 1044
            }
            else
            {

#line 1044
                bent_weight_1 = sin_half_0;

#line 1044
            }



            float cos_turn_0 = cos_half_0 * cos_gamma_1 + bent_weight_1 * sin_gamma_1;

#line 1059
            float bent_weight_2 = bent_weight_0 + _S41;

#line 1059
            bent_1 = bent_0 + (normal_0 * float3(cos_turn_0)  - cross(axis_0, normal_0) * float3((bent_weight_1 * cos_gamma_1 - cos_half_0 * sin_gamma_1))  + axis_0 * float3((_S38 * (1.0f - cos_turn_0))) ) * float3(_S41) ;

#line 1059
            bent_weight_1 = bent_weight_2;

#line 1029
        }
        else
        {

#line 1029
            bent_1 = bent_0;

#line 1029
            bent_weight_1 = bent_weight_0;

#line 1029
        }

#line 1029
        visibility_0 = visibility_1;

#line 1029
        weight_0 = weight_1;

#line 1029
        bent_0 = bent_1;

#line 1029
        bent_weight_0 = bent_weight_1;

#line 947
        slice_1 = slice_1 + 1U;

#line 947
    }

#line 1063
    if(weight_0 <= 0.0f)
    {
        return unoccluded_0;
    }
    float occlusion_0 = saturate(1.0f - visibility_0 / weight_0);



    if(bent_weight_0 <= 0.0f)
    {

#line 1071
        _S33 = true;

#line 1071
    }
    else
    {

#line 1071
        _S33 = (length(bent_0 / float3(bent_weight_0) )) < 0.5f;

#line 1071
    }

#line 1071
    if(_S33)
    {
        return float4(occlusion_0, 0.0f, 0.0f, 0.0f);
    }



    return float4(occlusion_0, normalize((((float4(bent_0, 0.0f)) * (matrix<float,int(4),int(4)> (kernelContext_11->camera_0->inv_view_0.data_0[int(0)][int(0)], kernelContext_11->camera_0->inv_view_0.data_0[int(1)][int(0)], kernelContext_11->camera_0->inv_view_0.data_0[int(2)][int(0)], kernelContext_11->camera_0->inv_view_0.data_0[int(3)][int(0)], kernelContext_11->camera_0->inv_view_0.data_0[int(0)][int(1)], kernelContext_11->camera_0->inv_view_0.data_0[int(1)][int(1)], kernelContext_11->camera_0->inv_view_0.data_0[int(2)][int(1)], kernelContext_11->camera_0->inv_view_0.data_0[int(3)][int(1)], kernelContext_11->camera_0->inv_view_0.data_0[int(0)][int(2)], kernelContext_11->camera_0->inv_view_0.data_0[int(1)][int(2)], kernelContext_11->camera_0->inv_view_0.data_0[int(2)][int(2)], kernelContext_11->camera_0->inv_view_0.data_0[int(3)][int(2)], kernelContext_11->camera_0->inv_view_0.data_0[int(0)][int(3)], kernelContext_11->camera_0->inv_view_0.data_0[int(1)][int(3)], kernelContext_11->camera_0->inv_view_0.data_0[int(2)][int(3)], kernelContext_11->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz));
}


#line 1078
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1078
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 1102
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S42 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 1102
    thread KernelContext_0 kernelContext_12;

#line 1102
    (&kernelContext_12)->scene_depth_0 = scene_depth_1;

#line 1102
    (&kernelContext_12)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 1112
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_7 = int2(int(width_0), int(height_0));
    float2 size_3 = float2(float(width_0), float(height_0));

#line 1122
    int2 _S43 = int2(position_0.xy);
    int2 pixel_8 = full_res_pixel_0(_S43);
    uint tile_1 = (uint(_S43.y) & 3U) * 4U + (uint(_S43.x) & 3U);

#line 1124
    float _S44 = depth_at_0(pixel_8, extent_7, &kernelContext_12);



    if(_S44 <= 0.0f)
    {

#line 1128
        pixelOutput_0 _S45 = { float4(1.0f, encode_bent_0(float3(0.0f, 0.0f, 0.0f))) };

        return _S45;
    }

#line 1130
    float3 _S46 = view_position_0(pixel_8, _S44, size_3, &kernelContext_12);

#line 1130
    float3 _S47 = normal_at_0(pixel_8, _S46, extent_7, size_3, &kernelContext_12);

#line 1130
    float4 _S48 = occlusion_at_0(pixel_8, tile_1, _S46, _S47, extent_7, size_3, &kernelContext_12);

#line 1130
    pixelOutput_0 _S49 = { float4(saturate(1.0f - _S48.x), encode_bent_0(_S48.yzw)) };

#line 1136
    return _S49;
}


#line 1136
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 482
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 482
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 482
    thread KernelContext_0 kernelContext_13;

#line 482
    (&kernelContext_13)->scene_depth_0 = scene_depth_2;

#line 482
    (&kernelContext_13)->camera_0 = camera_2;

#line 1084
    thread FullscreenOutput_0 output_1;


    float2 _S50 = float2(float((index_1 << 1U) & 2U), float(index_1 & 2U));

#line 1087
    (&output_1)->uv_2 = _S50;
    (&output_1)->position_2 = float4(_S50 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 1088
    thread vertexMain_Result_0 _S51;

#line 1088
    (&_S51)->position_1 = output_1.position_2;

#line 1088
    (&_S51)->uv_1 = output_1.uv_2;

#line 1088
    return _S51;
}

