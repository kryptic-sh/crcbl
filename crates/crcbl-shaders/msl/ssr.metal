#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 344 "shaders/ssr.slang"
float sharpness_of_0(float roughness_0)
{
    return saturate(1.0f - roughness_0 / 0.5f);
}


#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 104 "shaders/ssr.slang"
struct SsrParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    uint4 probe_counts_0;
    uint4 probe_levels_0;
    array<float4, int(4)> probe_level_origin_0;
    array<float4, int(4)> probe_level_inv_spacing_0;
    array<uint4, int(4)> probe_level_offset_0;
    uint4 hiz_0;
    array<float4, int(3)> sky_0;
};


#line 1084 "core"
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 5516 "core.meta.slang"
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    texture2d<float, access::sample> reflectivity_0;
    SsrParams_natural_0 constant* camera_0;
    GpuProbe_natural_0 device* probes_0;
    texture2d_array<float, access::sample> probe_visibility_0;
    texture2d<float, access::sample> sky_prefilter_0;
    texture2d<float, access::sample> dfg_0;
    depth2d<float, access::sample> hiz_1_0;
    depth2d<float, access::sample> hiz_2_0;
    depth2d<float, access::sample> hiz_3_0;
    depth2d<float, access::sample> hiz_4_0;
    depth2d<float, access::sample> hiz_5_0;
    texture2d<float, access::sample> scene_color_0;
};


#line 490 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 493
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 490
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 493
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 511
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 511
float2 unproject_z_1(float depth_1, KernelContext_0 thread* kernelContext_3)
{
    return float2((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].z * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].w * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 542
float4 unproject_0(float2 ndc_0, float depth_2, KernelContext_0 thread* kernelContext_4)
{

#line 542
    float2 _S3 = unproject_z_0(depth_2, kernelContext_4);


    return float4((&kernelContext_4->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].y, _S3.x, _S3.y);
}


#line 558
float3 view_position_0(int2 pixel_2, float depth_3, float2 extent_2, KernelContext_0 thread* kernelContext_5)
{

#line 558
    float4 _S4 = unproject_0(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_3, kernelContext_5);

#line 569
    return _S4.xyz / float3(_S4.w) ;
}


#line 558
float3 view_position_1(int2 pixel_3, float depth_4, float2 extent_3, KernelContext_0 thread* kernelContext_6)
{

#line 558
    float4 _S5 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_4, kernelContext_6);

#line 569
    return _S5.xyz / float3(_S5.w) ;
}


#line 584
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_7)
{
    int2 _S6 = pixel_4 + int2(int(-1), int(0));

#line 586
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_7);

#line 586
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_7);
    int2 _S9 = pixel_4 + int2(int(1), int(0));

#line 587
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_7);

#line 587
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_7);
    int2 _S12 = pixel_4 + int2(int(0), int(-1));

#line 588
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_7);

#line 588
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_7);
    int2 _S15 = pixel_4 + int2(int(0), int(1));

#line 589
    float _S16 = depth_at_1(_S15, extent_4, kernelContext_7);

#line 589
    float3 _S17 = view_position_1(_S15, _S16, size_0, kernelContext_7);

    float _S18 = centre_0.z;

#line 591
    float3 horizontal_0;
    if((abs(_S11.z - _S18)) < (abs(_S18 - _S8.z)))
    {

#line 592
        horizontal_0 = _S11 - centre_0;

#line 592
    }
    else
    {

#line 592
        horizontal_0 = centre_0 - _S8;

#line 592
    }

#line 592
    float3 vertical_0;


    if((abs(_S17.z - _S18)) < (abs(_S18 - _S14.z)))
    {

#line 595
        vertical_0 = _S17 - centre_0;

#line 595
    }
    else
    {

#line 595
        vertical_0 = centre_0 - _S14;

#line 595
    }

#line 605
    return normalize(cross(vertical_0, horizontal_0));
}


#line 1011
float probe_level_reach_0(float3 world_position_0, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 1011
    float reach_0 = 0.0f;

#line 1011
    uint axis_0 = 0U;


    for(;;)
    {

#line 1014
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 1014
            break;
        }

#line 1014
        uint _S19 = axis_0;

#line 1014
        bool _S20;

        if((last_0[axis_0]) == 0.0f)
        {

#line 1016
            _S20 = true;

#line 1016
        }
        else
        {

#line 1016
            _S20 = (inv_spacing_0[axis_0]) == 0.0f;

#line 1016
        }

#line 1016
        if(_S20)
        {

#line 1017
            axis_0 = axis_0 + 1U;

#line 1014
            continue;
        }

#line 1014
        reach_0 = max(reach_0, abs(2.0f * ((world_position_0[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S19] - 1.0f));

#line 1014
        axis_0 = axis_0 + 1U;

#line 1014
    }

#line 1021
    return reach_0;
}


#line 1031
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 1031
    uint level_0 = 0U;

    for(;;)
    {

#line 1033
        uint _S21 = level_0 + 1U;

#line 1033
        if(_S21 < levels_0)
        {
        }
        else
        {

#line 1033
            break;
        }
        float _S22 = float(level_0);

#line 1035
        float at_0 = reach_1 * exp2(- _S22);
        if(at_0 < 1.0f)
        {

#line 1037
            return float2(_S22, saturate((1.0f - at_0) / 0.25f));
        }

#line 1033
        level_0 = _S21;

#line 1033
    }

#line 1039
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 920
uint probe_wrap_0(uint cell_0, uint offset_0, uint count_0)
{
    uint at_1 = cell_0 + offset_0;

#line 922
    uint _S23;
    if(at_1 >= count_0)
    {

#line 923
        _S23 = at_1 - count_0;

#line 923
    }
    else
    {

#line 923
        _S23 = at_1;

#line 923
    }

#line 923
    return _S23;
}


#line 936
uint probe_row_0(uint level_1, uint3 cell_1, KernelContext_0 thread* kernelContext_8)
{
    uint3 counts_0 = kernelContext_8->camera_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_8->camera_0->probe_level_offset_0[level_1].xyz;
    uint _S24 = counts_0.x;
    uint _S25 = counts_0.y;



    return min(kernelContext_8->camera_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_1.z, offset_1.z, counts_0.z) * _S25 + probe_wrap_0(cell_1.y, offset_1.y, _S25)) * _S24 + probe_wrap_0(cell_1.x, offset_1.x, _S24), max(kernelContext_8->camera_0->probe_counts_0.w, 1U) - 1U);
}


#line 822
float sign_not_zero_0(float value_0)
{

#line 822
    float _S26;

    if(value_0 >= 0.0f)
    {

#line 824
        _S26 = 1.0f;

#line 824
    }
    else
    {

#line 824
        _S26 = -1.0f;

#line 824
    }

#line 824
    return _S26;
}


#line 832
float2 oct_encode_0(float3 direction_0)
{
    float _S27 = direction_0.y;
    float2 p_0 = direction_0.xz / float2(max(abs(direction_0.x) + abs(_S27) + abs(direction_0.z), 9.99999968265522539e-21f)) ;

#line 835
    float2 p_1;
    if(_S27 < 0.0f)
    {
        float _S28 = p_0.y;

#line 838
        float _S29 = p_0.x;

#line 838
        p_1 = float2((1.0f - abs(_S28)) * sign_not_zero_0(_S29), (1.0f - abs(_S29)) * sign_not_zero_0(_S28));

#line 836
    }
    else
    {

#line 836
        p_1 = p_0;

#line 836
    }

#line 841
    return p_1;
}


#line 850
float2 probe_moments_0(uint index_0, float3 direction_1, KernelContext_0 thread* kernelContext_9)
{

#line 850
    texture2d_array<float, access::sample> _S30 = kernelContext_9->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S30).get_width(0)),(*((&height_0)) = (_S30).get_height(0)),(*((&layers_0)) = (_S30).get_array_size());

#line 855
    float2 _S31 = float2(0.5f) ;

#line 855
    float2 _S32 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_1) * _S31 + _S31) * float2(16.0f)  + _S32 - _S31;
    float2 _S33 = float2(float(width_0), float(height_0)) - _S32;

#line 859
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S33);
    float2 high_0 = min(low_0 + _S32, _S33);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_0, max(layers_0, 1U) - 1U));

    int _S34 = int(low_0.x);

#line 864
    int _S35 = int(low_0.y);

#line 864
    int4 _S36 = int4(_S34, _S35, layer_0, int(0));
    int _S37 = int(high_0.x);

#line 865
    int4 _S38 = int4(_S37, _S35, layer_0, int(0));
    int _S39 = int(high_0.y);

#line 866
    int4 _S40 = int4(_S34, _S39, layer_0, int(0));
    int4 _S41 = int4(_S37, _S39, layer_0, int(0));
    float2 _S42 = float2(weight_0.x) ;

#line 868
    return mix(mix(((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S36)).xy), uint(((_S36)).z), uint(((_S36)).w))).xy, ((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S38)).xy), uint(((_S38)).z), uint(((_S38)).w))).xy, _S42), mix(((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S40)).xy), uint(((_S40)).z), uint(((_S40)).w))).xy, ((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S41)).xy), uint(((_S41)).z), uint(((_S41)).w))).xy, _S42), float2(weight_0.y) );
}


#line 883
float probe_chebyshev_0(uint index_1, float3 probe_position_0, float3 world_position_1, float3 normal_0, KernelContext_0 thread* kernelContext_10)
{
    float3 to_probe_0 = probe_position_0 - (world_position_1 + normal_0 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 886
    float2 _S43 = probe_moments_0(index_1, - to_probe_0, kernelContext_10);

#line 892
    float _S44 = _S43.x;

#line 892
    float _S45 = max(_S43.y - _S44 * _S44, 0.0f);
    float behind_0 = to_surface_0 - _S44;
    float bound_0 = _S45 / (_S45 + behind_0 * behind_0);

#line 894
    float _S46;
    if(to_surface_0 <= _S44)
    {

#line 895
        _S46 = 1.0f;

#line 895
    }
    else
    {

#line 895
        _S46 = bound_0 * bound_0 * bound_0;

#line 895
    }

#line 895
    return _S46;
}


#line 911
float probe_weight_0(uint index_2, float3 probe_position_1, float3 world_position_2, float3 normal_1, KernelContext_0 thread* kernelContext_11)
{

#line 911
    float _S47 = probe_chebyshev_0(index_2, probe_position_1, world_position_2, normal_1, kernelContext_11);

    return max(_S47, 0.00009999999747379f);
}


#line 156
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 951
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 978
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_2, float3 origin_1, float3 spacing_0, float3 world_position_3, float3 normal_2, KernelContext_0 thread* kernelContext_12)
{

#line 979
    uint _S48 = probe_row_0(level_2, cell_2, kernelContext_12);


    GpuProbe_natural_0 stored_0 = kernelContext_12->probes_0[_S48];

#line 982
    float _S49 = probe_weight_0(_S48, origin_1 + float3(cell_2) * spacing_0, world_position_3, normal_2, kernelContext_12);



    thread WeightedProbe_0 corner_0;

#line 986
    float4 _S50 = float4(_S49) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S50;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S50;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S50;
    (&corner_0)->weight_1 = _S49;
    return corner_0;
}


#line 962
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S51 = float4(t_0) ;

#line 965
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S51);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S51);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S51);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1076
float3 probe_level_environment_0(uint level_3, float3 world_position_4, float3 normal_3, float3 direction_2, KernelContext_0 thread* kernelContext_13)
{

#line 1076
    float3 _S52 = float3(1.0f) ;

    float3 _S53 = float3(0.0f, 0.0f, 0.0f);

#line 1078
    float3 last_1 = max(float3(kernelContext_13->camera_0->probe_counts_0.xyz) - _S52, _S53);



    float3 origin_2 = kernelContext_13->camera_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_13->camera_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_4 - origin_2) * inv_0, _S53, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S54 = uint3(base_0);
    uint3 _S55 = uint3(min(base_0 + _S52, last_1));

#line 1093
    float _S56 = inv_0.x;

#line 1093
    float _S57;

#line 1093
    if(_S56 != 0.0f)
    {

#line 1093
        _S57 = 1.0f / _S56;

#line 1093
    }
    else
    {

#line 1093
        _S57 = 0.0f;

#line 1093
    }
    float _S58 = inv_0.y;

#line 1094
    float _S59;

#line 1094
    if(_S58 != 0.0f)
    {

#line 1094
        _S59 = 1.0f / _S58;

#line 1094
    }
    else
    {

#line 1094
        _S59 = 0.0f;

#line 1094
    }
    float _S60 = inv_0.z;

#line 1095
    float _S61;

#line 1095
    if(_S60 != 0.0f)
    {

#line 1095
        _S61 = 1.0f / _S60;

#line 1095
    }
    else
    {

#line 1095
        _S61 = 0.0f;

#line 1095
    }

#line 1093
    float3 spacing_1 = float3(_S57, _S59, _S61);

#line 1102
    uint _S62 = _S54.x;

#line 1102
    uint _S63 = _S54.y;

#line 1102
    uint _S64 = _S54.z;

#line 1102
    WeightedProbe_0 _S65 = probe_corner_0(level_3, uint3(_S62, _S63, _S64), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);
    uint _S66 = _S55.x;

#line 1103
    WeightedProbe_0 _S67 = probe_corner_0(level_3, uint3(_S66, _S63, _S64), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1103
    float _S68 = f_0.x;

#line 1103
    thread WeightedProbe_0 _S69 = _S65;

#line 1103
    thread WeightedProbe_0 _S70 = _S67;

#line 1103
    WeightedProbe_0 _S71 = lerp_probe_0(&_S69, &_S70, _S68);
    uint _S72 = _S55.y;

#line 1104
    WeightedProbe_0 _S73 = probe_corner_0(level_3, uint3(_S62, _S72, _S64), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1104
    WeightedProbe_0 _S74 = probe_corner_0(level_3, uint3(_S66, _S72, _S64), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1104
    thread WeightedProbe_0 _S75 = _S73;

#line 1104
    thread WeightedProbe_0 _S76 = _S74;

#line 1104
    WeightedProbe_0 _S77 = lerp_probe_0(&_S75, &_S76, _S68);

    uint _S78 = _S55.z;

#line 1106
    WeightedProbe_0 _S79 = probe_corner_0(level_3, uint3(_S62, _S63, _S78), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1106
    WeightedProbe_0 _S80 = probe_corner_0(level_3, uint3(_S66, _S63, _S78), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1106
    thread WeightedProbe_0 _S81 = _S79;

#line 1106
    thread WeightedProbe_0 _S82 = _S80;

#line 1106
    WeightedProbe_0 _S83 = lerp_probe_0(&_S81, &_S82, _S68);

#line 1106
    WeightedProbe_0 _S84 = probe_corner_0(level_3, uint3(_S62, _S72, _S78), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1106
    WeightedProbe_0 _S85 = probe_corner_0(level_3, uint3(_S66, _S72, _S78), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1106
    thread WeightedProbe_0 _S86 = _S84;

#line 1106
    thread WeightedProbe_0 _S87 = _S85;

#line 1106
    WeightedProbe_0 _S88 = lerp_probe_0(&_S86, &_S87, _S68);



    float _S89 = f_0.y;

#line 1110
    thread WeightedProbe_0 _S90 = _S71;

#line 1110
    thread WeightedProbe_0 _S91 = _S77;

#line 1110
    WeightedProbe_0 _S92 = lerp_probe_0(&_S90, &_S91, _S89);

#line 1110
    thread WeightedProbe_0 _S93 = _S83;

#line 1110
    thread WeightedProbe_0 _S94 = _S88;

#line 1110
    WeightedProbe_0 _S95 = lerp_probe_0(&_S93, &_S94, _S89);

    float _S96 = f_0.z;

#line 1112
    thread WeightedProbe_0 _S97 = _S92;

#line 1112
    thread WeightedProbe_0 _S98 = _S95;

#line 1112
    WeightedProbe_0 _S99 = lerp_probe_0(&_S97, &_S98, _S96);

#line 1112
    float3 _S100 = float3(2.09439516067504883f) ;

#line 1118
    return max(float3(dot(_S99.sh_0.sh_r_0.xyz / _S100, direction_2) + _S99.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S99.sh_0.sh_g_0.xyz / _S100, direction_2) + _S99.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S99.sh_0.sh_b_0.xyz / _S100, direction_2) + _S99.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S99.weight_1) , _S53);
}


#line 1135
float3 probe_environment_0(float3 world_position_5, float3 normal_4, float3 direction_3, KernelContext_0 thread* kernelContext_14)
{

#line 1143
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_5, kernelContext_14->camera_0->probe_level_origin_0[int(0)].xyz, kernelContext_14->camera_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_14->camera_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_14->camera_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1145
    float3 _S101 = probe_level_environment_0(level_4, world_position_5, normal_4, direction_3, kernelContext_14);


    if(share_0 >= 1.0f)
    {

#line 1149
        return _S101;
    }

#line 1149
    float3 _S102 = probe_level_environment_0(level_4 + 1U, world_position_5, normal_4, direction_3, kernelContext_14);

    return _S102 * float3((1.0f - share_0))  + _S101 * float3(share_0) ;
}


#line 751
float2 decode_fixed_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 763
float2 fixed_pair_at_0(texture2d<float, access::sample> table_0, float2 at_2)
{
    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (table_0).get_width(0)),(*((&height_1)) = (table_0).get_height(0));
    float2 extent_5 = float2(float(width_1), float(height_1));
    float2 scaled_1 = saturate(at_2) * extent_5 - float2(0.5f) ;

#line 769
    float2 _S103 = float2(1.0f) ;
    float2 _S104 = extent_5 - _S103;

#line 770
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S104);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S105 = int2(low_1);
    int2 _S106 = int2(min(low_1 + _S103, _S104));
    int _S107 = _S105.x;

#line 776
    int _S108 = _S105.y;

#line 776
    int3 _S109 = int3(_S107, _S108, int(0));
    int _S110 = _S106.x;

#line 777
    int3 _S111 = int3(_S110, _S108, int(0));
    float2 _S112 = float2(weight_2.x) ;
    int _S113 = _S106.y;

#line 779
    int3 _S114 = int3(_S107, _S113, int(0));
    int3 _S115 = int3(_S110, _S113, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S109)).xy), uint(((_S109)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S111)).xy), uint(((_S111)).z)))), _S112), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S114)).xy), uint(((_S114)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S115)).xy), uint(((_S115)).z)))), _S112), float2(weight_2.y) );
}


float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_15)
{
    return fixed_pair_at_0(kernelContext_15->sky_prefilter_0, float2(up_0, roughness_1));
}


#line 809
float3 sky_prefiltered_0(float3 direction_4, float roughness_2, KernelContext_0 thread* kernelContext_16)
{
    float up_1 = clamp(direction_4.y, -1.0f, 1.0f);

#line 811
    float2 _S116 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_16);

    bool _S117 = up_1 >= 0.0f;

#line 813
    float3 far_0;

#line 813
    if(_S117)
    {

#line 813
        far_0 = kernelContext_16->camera_0->sky_0[int(0)].xyz;

#line 813
    }
    else
    {

#line 813
        far_0 = kernelContext_16->camera_0->sky_0[int(2)].xyz;

#line 813
    }

#line 813
    float3 opposite_0;
    if(_S117)
    {

#line 814
        opposite_0 = kernelContext_16->camera_0->sky_0[int(2)].xyz;

#line 814
    }
    else
    {

#line 814
        opposite_0 = kernelContext_16->camera_0->sky_0[int(0)].xyz;

#line 814
    }
    float _S118 = _S116.x;

#line 815
    float _S119 = _S116.y;
    return kernelContext_16->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S118 - _S119))  + far_0 * float3(_S118)  + opposite_0 * float3(_S119) ;
}


#line 792
float2 dfg_at_0(float n_dot_v_0, float roughness_3, KernelContext_0 thread* kernelContext_17)
{
    return fixed_pair_at_0(kernelContext_17->dfg_0, float2(n_dot_v_0, roughness_3));
}


#line 614
float2 pixel_of_0(float2 ndc_1, float2 size_1)
{
    return float2((ndc_1.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_1.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_3, float2 size_2)
{
    return float2(at_3.x / size_2.x * 2.0f - 1.0f, 1.0f - at_3.y / size_2.y * 2.0f);
}


#line 691
float cell_exit_0(float2 at_4, float2 forward_0, float size_3, float reach_2)
{

    float _S120 = forward_0.x;

#line 694
    bool _S121 = _S120 > 0.0f;

#line 694
    float along_x_0;

#line 694
    if(_S121)
    {

#line 694
        along_x_0 = (floor(at_4.x / size_3) + 1.0f) * size_3;

#line 694
    }
    else
    {

#line 694
        along_x_0 = floor(at_4.x / size_3) * size_3;

#line 694
    }
    float _S122 = forward_0.y;

#line 695
    bool _S123 = _S122 > 0.0f;

#line 695
    float along_y_0;

#line 695
    if(_S123)
    {

#line 695
        along_y_0 = (floor(at_4.y / size_3) + 1.0f) * size_3;

#line 695
    }
    else
    {

#line 695
        along_y_0 = floor(at_4.y / size_3) * size_3;

#line 695
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 696
    float _S124;

    if((abs(_S120)) < 9.99999997475242708e-07f)
    {

#line 698
        along_x_0 = reach_2;

#line 698
    }
    else
    {

#line 699
        if(_S121)
        {

#line 699
            _S124 = nudge_0;

#line 699
        }
        else
        {

#line 699
            _S124 = - nudge_0;

#line 699
        }

#line 699
        along_x_0 = (along_x_0 + _S124 - at_4.x) / _S120;

#line 698
    }


    if((abs(_S122)) < 9.99999997475242708e-07f)
    {

#line 701
        along_y_0 = reach_2;

#line 701
    }
    else
    {

#line 702
        if(_S123)
        {

#line 702
            _S124 = nudge_0;

#line 702
        }
        else
        {

#line 702
            _S124 = - nudge_0;

#line 702
        }

#line 702
        along_y_0 = (along_y_0 + _S124 - at_4.y) / _S122;

#line 701
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 650
float hiz_at_0(uint level_5, int2 texel_1, int2 extent_6, KernelContext_0 thread* kernelContext_18)
{
    int2 _S125 = int2(int(0), int(0));
    int3 at_5 = int3(clamp(texel_1, _S125, max(extent_6 - int2(int(1), int(1)), _S125)), int(0));
    switch(level_5)
    {
    case 0U:
        {

#line 657
            return ((kernelContext_18->scene_depth_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    case 1U:
        {

#line 659
            return ((kernelContext_18->hiz_1_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    case 2U:
        {

#line 661
            return ((kernelContext_18->hiz_2_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    case 3U:
        {

#line 663
            return ((kernelContext_18->hiz_3_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    case 4U:
        {

#line 665
            return ((kernelContext_18->hiz_4_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    default:
        {

#line 667
            return ((kernelContext_18->hiz_5_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    }

#line 667
}


#line 678
float view_z_of_0(float depth_5, KernelContext_0 thread* kernelContext_19)
{

#line 678
    float2 _S126 = unproject_z_1(depth_5, kernelContext_19);


    return _S126.x / _S126.y;
}


#line 633
float thickness_at_0(float advance_0, float depth_6)
{
    return max(advance_0, abs(depth_6) * 0.01999999955296516f);
}


#line 635
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 635
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 1166
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S127 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], texture2d<float, access::sample> dfg_1 [[texture(9)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 1166
    float3 reflection_0;

#line 1166
    thread KernelContext_0 kernelContext_20;

#line 1166
    (&kernelContext_20)->scene_depth_0 = scene_depth_1;

#line 1166
    (&kernelContext_20)->reflectivity_0 = reflectivity_1;

#line 1166
    (&kernelContext_20)->camera_0 = camera_1;

#line 1166
    (&kernelContext_20)->probes_0 = probes_1;

#line 1166
    (&kernelContext_20)->probe_visibility_0 = probe_visibility_1;

#line 1166
    (&kernelContext_20)->sky_prefilter_0 = sky_prefilter_1;

#line 1166
    (&kernelContext_20)->dfg_0 = dfg_1;

#line 1166
    (&kernelContext_20)->hiz_1_0 = hiz_1_1;

#line 1166
    (&kernelContext_20)->hiz_2_0 = hiz_2_1;

#line 1166
    (&kernelContext_20)->hiz_3_0 = hiz_3_1;

#line 1166
    (&kernelContext_20)->hiz_4_0 = hiz_4_1;

#line 1166
    (&kernelContext_20)->hiz_5_0 = hiz_5_1;

#line 1166
    (&kernelContext_20)->scene_color_0 = scene_color_1;

    thread uint width_2;
    thread uint height_2;



    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    int _S128 = int(width_2);

#line 1174
    int _S129 = int(height_2);

#line 1174
    int2 extent_7 = int2(_S128, _S129);
    float _S130 = float(width_2);

#line 1175
    float _S131 = float(height_2);

#line 1175
    float2 size_4 = float2(_S130, _S131);
    int2 _S132 = int2(position_0.xy);

#line 1183
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S133 = int3(_S132, int(0));

#line 1185
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S133)).xy), uint(((_S133)).z)));
    float _S134 = surface_0.w;

#line 1186
    float sharpness_0 = sharpness_of_0(_S134);

#line 1186
    float _S135 = depth_at_0(_S132, extent_7, &kernelContext_20);


    if(_S135 <= 0.0f)
    {

#line 1189
        pixelOutput_0 _S136 = { NOTHING_0 };

        return _S136;
    }

#line 1191
    float3 _S137 = view_position_0(_S132, _S135, size_4, &kernelContext_20);

#line 1191
    float3 _S138 = normal_at_0(_S132, _S137, extent_7, size_4, &kernelContext_20);

#line 1197
    float3 towards_0 = normalize(_S137);
    float3 ray_0 = reflect(towards_0, _S138);


    float4 _S139 = float4(ray_0, 0.0f);

#line 1201
    float3 reflection_direction_0 = normalize((((_S139) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 1201
    float3 _S140 = probe_environment_0((((float4(_S137, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, normalize((((float4(_S138, 0.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz), reflection_direction_0, &kernelContext_20);

#line 1201
    float3 _S141 = sky_prefiltered_0(reflection_direction_0, _S134, &kernelContext_20);

#line 1221
    float3 environment_0 = _S140 + _S141;

#line 1229
    float3 _S142 = - towards_0;
    float3 f0_0 = surface_0.xyz;

#line 1230
    float2 _S143 = dfg_at_0(saturate(dot(_S138, _S142)), _S134, &kernelContext_20);

    float3 env_brdf_0 = f0_0 * float3(_S143.x)  + float3(_S143.y) ;

#line 1237
    if(sharpness_0 <= 0.0f)
    {

#line 1237
        pixelOutput_0 _S144 = { float4(environment_0 * env_brdf_0, 0.0f) };

        return _S144;
    }


    float _S145 = saturate((1.0f - dot(ray_0, _S142)) / 0.05000000074505806f);


    float _S146 = _S137.z;

#line 1246
    float3 start_0 = _S137 + _S138 * float3((abs(_S146) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S139) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S147 = clip_start_0.w;

#line 1251
    if(_S147 <= 0.0f)
    {

#line 1251
        pixelOutput_0 _S148 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S148;
    }
    float2 _S149 = clip_start_0.xy;

#line 1255
    float2 _S150 = float2(_S147) ;

#line 1255
    float2 at_start_0 = pixel_of_0(_S149 / _S150, size_4);

#line 1261
    float2 _S151 = clip_ray_0.xy;

#line 1261
    float _S152 = clip_ray_0.w;

#line 1261
    float2 _S153 = float2(_S152) ;

#line 1261
    float2 ndc_rate_0 = (_S151 * _S150 - _S149 * _S153) / float2((_S147 * _S147)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S130, - ndc_rate_0.y * 0.5f * _S131);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 1264
        pixelOutput_0 _S154 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S154;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 1275
    float reach_3 = 0.75f * min(_S130, _S131);

    float _S155 = forward_1.x;

#line 1277
    float travel_0;

#line 1277
    if(_S155 > 0.0f)
    {

#line 1277
        travel_0 = min(reach_3, (_S130 - 1.0f - at_start_0.x) / _S155);

#line 1277
    }
    else
    {

        if(_S155 < 0.0f)
        {

#line 1281
            travel_0 = min(reach_3, - at_start_0.x / _S155);

#line 1281
        }
        else
        {

#line 1281
            travel_0 = reach_3;

#line 1281
        }

#line 1277
    }

#line 1285
    float _S156 = forward_1.y;

#line 1285
    if(_S156 > 0.0f)
    {

#line 1285
        travel_0 = min(travel_0, (_S131 - 1.0f - at_start_0.y) / _S156);

#line 1285
    }
    else
    {

        if(_S156 < 0.0f)
        {

#line 1289
            travel_0 = min(travel_0, - at_start_0.y / _S156);

#line 1289
        }

#line 1285
    }

#line 1297
    if(_S152 > 0.0f)
    {

#line 1297
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S151 / _S153, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 1297
    }
    else
    {

#line 1312
        if(_S152 < 0.0f)
        {

#line 1319
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 1324
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S147) / _S152)) ;

#line 1324
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 1312
        }

#line 1297
    }

#line 1331
    float _S157 = max(travel_0, 0.0f);
    if(_S157 <= 0.00390625f)
    {

#line 1332
        pixelOutput_0 _S158 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S158;
    }

#line 1341
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S157) , size_4);

#line 1341
    float when_end_0;

    if((abs(_S155)) >= (abs(_S156)))
    {

#line 1343
        float _S159 = ndc_end_0.x;

#line 1343
        when_end_0 = (_S159 * _S147 - clip_start_0.x) / (clip_ray_0.x - _S159 * _S152);

#line 1343
    }
    else
    {

#line 1344
        float _S160 = ndc_end_0.y;

#line 1344
        when_end_0 = (_S160 * _S147 - clip_start_0.y) / (clip_ray_0.y - _S160 * _S152);

#line 1343
    }

#line 1343
    bool _S161;

#line 1351
    if(!(when_end_0 > 0.0f))
    {

#line 1351
        _S161 = true;

#line 1351
    }
    else
    {

#line 1351
        _S161 = !isfinite(when_end_0);

#line 1351
    }

#line 1351
    if(_S161)
    {

#line 1351
        pixelOutput_0 _S162 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S162;
    }

#line 1359
    float inverse_w_start_0 = 1.0f / _S147;

    float inverse_w_end_0 = 1.0f / (_S147 + when_end_0 * _S152);
    float _S163 = start_0.z;

#line 1362
    float _S164 = _S163 * inverse_w_start_0;
    float _S165 = (_S163 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 1368
    float3 _S166 = environment_0 * env_brdf_0;
    uint _S167 = min((&kernelContext_20)->camera_0->hiz_0.x, 5U);

#line 1399
    float _S168 = _S163 - _S146;

#line 1399
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S157), _S157);

#line 1399
    float previous_gap_0 = _S168;

#line 1399
    float entry_z_0 = _S163;

#line 1399
    uint step_0 = 0U;

#line 1399
    uint level_6 = 0U;

    for(;;)
    {

#line 1401
        if(step_0 < 96U)
        {
        }
        else
        {

#line 1401
            reflection_0 = _S166;

#line 1401
            break;
        }
        float cell_3 = float(1U << level_6);
        float2 at_6 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S169 = min(at_travel_0 + cell_exit_0(at_6, forward_1, cell_3, _S157), _S157);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S169) ;
        float along_0 = _S169 / _S157;

        float exit_z_0 = mix(_S164, _S165, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 1409
        float _S170 = hiz_at_0(level_6, int2(floor(at_6 / float2(cell_3) )), int2(_S128 >> level_6, _S129 >> level_6), &kernelContext_20);

#line 1409
        float gap_0;

#line 1418
        if(_S170 <= 0.0f)
        {

#line 1418
            gap_0 = 1.0f;

#line 1418
        }
        else
        {

#line 1418
            float _S171 = view_z_of_0(_S170, &kernelContext_20);

#line 1418
            gap_0 = exit_z_0 - _S171;

#line 1418
        }

#line 1427
        bool _S172 = !(gap_0 > 0.0f);

#line 1427
        if(_S172)
        {

#line 1427
            _S161 = level_6 > 0U;

#line 1427
        }
        else
        {

#line 1427
            _S161 = false;

#line 1427
        }

#line 1427
        if(_S161)
        {

#line 1427
            level_6 = level_6 - 1U;

#line 1433
            step_0 = step_0 + 1U;

#line 1401
            continue;
        }

#line 1401
        bool _S173;

#line 1436
        if(_S172)
        {

#line 1436
            _S173 = previous_gap_0 > 0.0f;

#line 1436
        }
        else
        {

#line 1436
            _S173 = false;

#line 1436
        }

#line 1436
        if(_S173)
        {



            float behind_1 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_1 <= thickness_0)
            {

#line 1449
                float2 hit_at_0 = mix(at_6, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1464
                float confidence_0 = sharpness_0 * _S145 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S169 / reach_3) / 0.25f) * saturate(1.0f - behind_1 / thickness_0);
                int3 _S174 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_7 - int2(int(1), int(1))), int(0));

#line 1465
                reflection_0 = (((&kernelContext_20)->scene_color_0).read(vec<uint,2>(((_S174)).xy), uint(((_S174)).z))).xyz * env_brdf_0 * float3(confidence_0)  + _S166 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 1436
        }

#line 1477
        if(_S169 >= _S157)
        {

#line 1477
            reflection_0 = _S166;

            break;
        }



        uint _S175 = min(level_6 + 1U, _S167);

#line 1484
        at_travel_0 = _S169;

#line 1484
        previous_gap_0 = gap_0;

#line 1484
        entry_z_0 = exit_z_0;

#line 1484
        level_6 = _S175;

#line 1401
        step_0 = step_0 + 1U;

#line 1401
    }

#line 1401
    pixelOutput_0 _S176 = { float4(reflection_0, sharpness_0) };

#line 1492
    return _S176;
}


#line 1492
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 478
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 478
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(8)]], texture2d<float, access::sample> dfg_2 [[texture(9)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 478
    thread KernelContext_0 kernelContext_21;

#line 478
    (&kernelContext_21)->scene_depth_0 = scene_depth_2;

#line 478
    (&kernelContext_21)->reflectivity_0 = reflectivity_2;

#line 478
    (&kernelContext_21)->camera_0 = camera_2;

#line 478
    (&kernelContext_21)->probes_0 = probes_2;

#line 478
    (&kernelContext_21)->probe_visibility_0 = probe_visibility_2;

#line 478
    (&kernelContext_21)->sky_prefilter_0 = sky_prefilter_2;

#line 478
    (&kernelContext_21)->dfg_0 = dfg_2;

#line 478
    (&kernelContext_21)->hiz_1_0 = hiz_1_2;

#line 478
    (&kernelContext_21)->hiz_2_0 = hiz_2_2;

#line 478
    (&kernelContext_21)->hiz_3_0 = hiz_3_2;

#line 478
    (&kernelContext_21)->hiz_4_0 = hiz_4_2;

#line 478
    (&kernelContext_21)->hiz_5_0 = hiz_5_2;

#line 478
    (&kernelContext_21)->scene_color_0 = scene_color_2;

#line 1157
    thread FullscreenOutput_0 output_1;


    float2 _S177 = float2(float((index_3 << 1U) & 2U), float(index_3 & 2U));

#line 1160
    (&output_1)->uv_2 = _S177;
    (&output_1)->position_2 = float4(_S177 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 1161
    thread vertexMain_Result_0 _S178;

#line 1161
    (&_S178)->position_1 = output_1.position_2;

#line 1161
    (&_S178)->uv_1 = output_1.uv_2;

#line 1161
    return _S178;
}

