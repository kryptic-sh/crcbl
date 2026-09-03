#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 340 "shaders/ssr.slang"
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


#line 486 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 489
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 486
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 489
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 507
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 507
float2 unproject_z_1(float depth_1, KernelContext_0 thread* kernelContext_3)
{
    return float2((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].z * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].w * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 538
float4 unproject_0(float2 ndc_0, float depth_2, KernelContext_0 thread* kernelContext_4)
{

#line 538
    float2 _S3 = unproject_z_0(depth_2, kernelContext_4);


    return float4((&kernelContext_4->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].y, _S3.x, _S3.y);
}


#line 554
float3 view_position_0(int2 pixel_2, float depth_3, float2 extent_2, KernelContext_0 thread* kernelContext_5)
{

#line 554
    float4 _S4 = unproject_0(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_3, kernelContext_5);

#line 565
    return _S4.xyz / float3(_S4.w) ;
}


#line 554
float3 view_position_1(int2 pixel_3, float depth_4, float2 extent_3, KernelContext_0 thread* kernelContext_6)
{

#line 554
    float4 _S5 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_4, kernelContext_6);

#line 565
    return _S5.xyz / float3(_S5.w) ;
}


#line 580
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_7)
{
    int2 _S6 = pixel_4 + int2(int(-1), int(0));

#line 582
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_7);

#line 582
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_7);
    int2 _S9 = pixel_4 + int2(int(1), int(0));

#line 583
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_7);

#line 583
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_7);
    int2 _S12 = pixel_4 + int2(int(0), int(-1));

#line 584
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_7);

#line 584
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_7);
    int2 _S15 = pixel_4 + int2(int(0), int(1));

#line 585
    float _S16 = depth_at_1(_S15, extent_4, kernelContext_7);

#line 585
    float3 _S17 = view_position_1(_S15, _S16, size_0, kernelContext_7);

    float _S18 = centre_0.z;

#line 587
    float3 horizontal_0;
    if((abs(_S11.z - _S18)) < (abs(_S18 - _S8.z)))
    {

#line 588
        horizontal_0 = _S11 - centre_0;

#line 588
    }
    else
    {

#line 588
        horizontal_0 = centre_0 - _S8;

#line 588
    }

#line 588
    float3 vertical_0;


    if((abs(_S17.z - _S18)) < (abs(_S18 - _S14.z)))
    {

#line 591
        vertical_0 = _S17 - centre_0;

#line 591
    }
    else
    {

#line 591
        vertical_0 = centre_0 - _S14;

#line 591
    }

#line 601
    return normalize(cross(vertical_0, horizontal_0));
}


#line 989
float probe_level_reach_0(float3 world_position_0, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 989
    float reach_0 = 0.0f;

#line 989
    uint axis_0 = 0U;


    for(;;)
    {

#line 992
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 992
            break;
        }

#line 992
        uint _S19 = axis_0;

#line 992
        bool _S20;

        if((last_0[axis_0]) == 0.0f)
        {

#line 994
            _S20 = true;

#line 994
        }
        else
        {

#line 994
            _S20 = (inv_spacing_0[axis_0]) == 0.0f;

#line 994
        }

#line 994
        if(_S20)
        {

#line 995
            axis_0 = axis_0 + 1U;

#line 992
            continue;
        }

#line 992
        reach_0 = max(reach_0, abs(2.0f * ((world_position_0[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S19] - 1.0f));

#line 992
        axis_0 = axis_0 + 1U;

#line 992
    }

#line 999
    return reach_0;
}


#line 1009
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 1009
    uint level_0 = 0U;

    for(;;)
    {

#line 1011
        uint _S21 = level_0 + 1U;

#line 1011
        if(_S21 < levels_0)
        {
        }
        else
        {

#line 1011
            break;
        }
        float _S22 = float(level_0);

#line 1013
        float at_0 = reach_1 * exp2(- _S22);
        if(at_0 < 1.0f)
        {

#line 1015
            return float2(_S22, saturate((1.0f - at_0) / 0.25f));
        }

#line 1011
        level_0 = _S21;

#line 1011
    }

#line 1017
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 919
uint probe_row_0(uint level_1, uint3 cell_0, KernelContext_0 thread* kernelContext_8)
{


    return min(kernelContext_8->camera_0->probe_levels_0.y * level_1 + (cell_0.z * kernelContext_8->camera_0->probe_counts_0.y + cell_0.y) * kernelContext_8->camera_0->probe_counts_0.x + cell_0.x, max(kernelContext_8->camera_0->probe_counts_0.w, 1U) - 1U);
}


#line 818
float sign_not_zero_0(float value_0)
{

#line 818
    float _S23;

    if(value_0 >= 0.0f)
    {

#line 820
        _S23 = 1.0f;

#line 820
    }
    else
    {

#line 820
        _S23 = -1.0f;

#line 820
    }

#line 820
    return _S23;
}


#line 828
float2 oct_encode_0(float3 direction_0)
{
    float _S24 = direction_0.y;
    float2 p_0 = direction_0.xz / float2(max(abs(direction_0.x) + abs(_S24) + abs(direction_0.z), 9.99999968265522539e-21f)) ;

#line 831
    float2 p_1;
    if(_S24 < 0.0f)
    {
        float _S25 = p_0.y;

#line 834
        float _S26 = p_0.x;

#line 834
        p_1 = float2((1.0f - abs(_S25)) * sign_not_zero_0(_S26), (1.0f - abs(_S26)) * sign_not_zero_0(_S25));

#line 832
    }
    else
    {

#line 832
        p_1 = p_0;

#line 832
    }

#line 837
    return p_1;
}


#line 846
float2 probe_moments_0(uint index_0, float3 direction_1, KernelContext_0 thread* kernelContext_9)
{

#line 846
    texture2d_array<float, access::sample> _S27 = kernelContext_9->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S27).get_width(0)),(*((&height_0)) = (_S27).get_height(0)),(*((&layers_0)) = (_S27).get_array_size());

#line 851
    float2 _S28 = float2(0.5f) ;

#line 851
    float2 _S29 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_1) * _S28 + _S28) * float2(16.0f)  + _S29 - _S28;
    float2 _S30 = float2(float(width_0), float(height_0)) - _S29;

#line 855
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S30);
    float2 high_0 = min(low_0 + _S29, _S30);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_0, max(layers_0, 1U) - 1U));

    int _S31 = int(low_0.x);

#line 860
    int _S32 = int(low_0.y);

#line 860
    int4 _S33 = int4(_S31, _S32, layer_0, int(0));
    int _S34 = int(high_0.x);

#line 861
    int4 _S35 = int4(_S34, _S32, layer_0, int(0));
    int _S36 = int(high_0.y);

#line 862
    int4 _S37 = int4(_S31, _S36, layer_0, int(0));
    int4 _S38 = int4(_S34, _S36, layer_0, int(0));
    float2 _S39 = float2(weight_0.x) ;

#line 864
    return mix(mix(((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S33)).xy), uint(((_S33)).z), uint(((_S33)).w))).xy, ((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S35)).xy), uint(((_S35)).z), uint(((_S35)).w))).xy, _S39), mix(((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S37)).xy), uint(((_S37)).z), uint(((_S37)).w))).xy, ((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S38)).xy), uint(((_S38)).z), uint(((_S38)).w))).xy, _S39), float2(weight_0.y) );
}


#line 879
float probe_chebyshev_0(uint index_1, float3 probe_position_0, float3 world_position_1, float3 normal_0, KernelContext_0 thread* kernelContext_10)
{
    float3 to_probe_0 = probe_position_0 - (world_position_1 + normal_0 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 882
    float2 _S40 = probe_moments_0(index_1, - to_probe_0, kernelContext_10);

#line 888
    float _S41 = _S40.x;

#line 888
    float _S42 = max(_S40.y - _S41 * _S41, 0.0f);
    float behind_0 = to_surface_0 - _S41;
    float bound_0 = _S42 / (_S42 + behind_0 * behind_0);

#line 890
    float _S43;
    if(to_surface_0 <= _S41)
    {

#line 891
        _S43 = 1.0f;

#line 891
    }
    else
    {

#line 891
        _S43 = bound_0 * bound_0 * bound_0;

#line 891
    }

#line 891
    return _S43;
}


#line 907
float probe_weight_0(uint index_2, float3 probe_position_1, float3 world_position_2, float3 normal_1, KernelContext_0 thread* kernelContext_11)
{

#line 907
    float _S44 = probe_chebyshev_0(index_2, probe_position_1, world_position_2, normal_1, kernelContext_11);

    return max(_S44, 0.00009999999747379f);
}


#line 152
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 929
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 956
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_1, float3 origin_1, float3 spacing_0, float3 world_position_3, float3 normal_2, KernelContext_0 thread* kernelContext_12)
{

#line 957
    uint _S45 = probe_row_0(level_2, cell_1, kernelContext_12);


    GpuProbe_natural_0 stored_0 = kernelContext_12->probes_0[_S45];

#line 960
    float _S46 = probe_weight_0(_S45, origin_1 + float3(cell_1) * spacing_0, world_position_3, normal_2, kernelContext_12);



    thread WeightedProbe_0 corner_0;

#line 964
    float4 _S47 = float4(_S46) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S47;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S47;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S47;
    (&corner_0)->weight_1 = _S46;
    return corner_0;
}


#line 940
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S48 = float4(t_0) ;

#line 943
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S48);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S48);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S48);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1054
float3 probe_level_environment_0(uint level_3, float3 world_position_4, float3 normal_3, float3 direction_2, KernelContext_0 thread* kernelContext_13)
{

#line 1054
    float3 _S49 = float3(1.0f) ;

    float3 _S50 = float3(0.0f, 0.0f, 0.0f);

#line 1056
    float3 last_1 = max(float3(kernelContext_13->camera_0->probe_counts_0.xyz) - _S49, _S50);



    float3 origin_2 = kernelContext_13->camera_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_13->camera_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_4 - origin_2) * inv_0, _S50, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S51 = uint3(base_0);
    uint3 _S52 = uint3(min(base_0 + _S49, last_1));

#line 1071
    float _S53 = inv_0.x;

#line 1071
    float _S54;

#line 1071
    if(_S53 != 0.0f)
    {

#line 1071
        _S54 = 1.0f / _S53;

#line 1071
    }
    else
    {

#line 1071
        _S54 = 0.0f;

#line 1071
    }
    float _S55 = inv_0.y;

#line 1072
    float _S56;

#line 1072
    if(_S55 != 0.0f)
    {

#line 1072
        _S56 = 1.0f / _S55;

#line 1072
    }
    else
    {

#line 1072
        _S56 = 0.0f;

#line 1072
    }
    float _S57 = inv_0.z;

#line 1073
    float _S58;

#line 1073
    if(_S57 != 0.0f)
    {

#line 1073
        _S58 = 1.0f / _S57;

#line 1073
    }
    else
    {

#line 1073
        _S58 = 0.0f;

#line 1073
    }

#line 1071
    float3 spacing_1 = float3(_S54, _S56, _S58);

#line 1080
    uint _S59 = _S51.x;

#line 1080
    uint _S60 = _S51.y;

#line 1080
    uint _S61 = _S51.z;

#line 1080
    WeightedProbe_0 _S62 = probe_corner_0(level_3, uint3(_S59, _S60, _S61), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);
    uint _S63 = _S52.x;

#line 1081
    WeightedProbe_0 _S64 = probe_corner_0(level_3, uint3(_S63, _S60, _S61), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1081
    float _S65 = f_0.x;

#line 1081
    thread WeightedProbe_0 _S66 = _S62;

#line 1081
    thread WeightedProbe_0 _S67 = _S64;

#line 1081
    WeightedProbe_0 _S68 = lerp_probe_0(&_S66, &_S67, _S65);
    uint _S69 = _S52.y;

#line 1082
    WeightedProbe_0 _S70 = probe_corner_0(level_3, uint3(_S59, _S69, _S61), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1082
    WeightedProbe_0 _S71 = probe_corner_0(level_3, uint3(_S63, _S69, _S61), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1082
    thread WeightedProbe_0 _S72 = _S70;

#line 1082
    thread WeightedProbe_0 _S73 = _S71;

#line 1082
    WeightedProbe_0 _S74 = lerp_probe_0(&_S72, &_S73, _S65);

    uint _S75 = _S52.z;

#line 1084
    WeightedProbe_0 _S76 = probe_corner_0(level_3, uint3(_S59, _S60, _S75), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1084
    WeightedProbe_0 _S77 = probe_corner_0(level_3, uint3(_S63, _S60, _S75), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1084
    thread WeightedProbe_0 _S78 = _S76;

#line 1084
    thread WeightedProbe_0 _S79 = _S77;

#line 1084
    WeightedProbe_0 _S80 = lerp_probe_0(&_S78, &_S79, _S65);

#line 1084
    WeightedProbe_0 _S81 = probe_corner_0(level_3, uint3(_S59, _S69, _S75), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1084
    WeightedProbe_0 _S82 = probe_corner_0(level_3, uint3(_S63, _S69, _S75), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1084
    thread WeightedProbe_0 _S83 = _S81;

#line 1084
    thread WeightedProbe_0 _S84 = _S82;

#line 1084
    WeightedProbe_0 _S85 = lerp_probe_0(&_S83, &_S84, _S65);



    float _S86 = f_0.y;

#line 1088
    thread WeightedProbe_0 _S87 = _S68;

#line 1088
    thread WeightedProbe_0 _S88 = _S74;

#line 1088
    WeightedProbe_0 _S89 = lerp_probe_0(&_S87, &_S88, _S86);

#line 1088
    thread WeightedProbe_0 _S90 = _S80;

#line 1088
    thread WeightedProbe_0 _S91 = _S85;

#line 1088
    WeightedProbe_0 _S92 = lerp_probe_0(&_S90, &_S91, _S86);

    float _S93 = f_0.z;

#line 1090
    thread WeightedProbe_0 _S94 = _S89;

#line 1090
    thread WeightedProbe_0 _S95 = _S92;

#line 1090
    WeightedProbe_0 _S96 = lerp_probe_0(&_S94, &_S95, _S93);

#line 1090
    float3 _S97 = float3(2.09439516067504883f) ;

#line 1096
    return max(float3(dot(_S96.sh_0.sh_r_0.xyz / _S97, direction_2) + _S96.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S96.sh_0.sh_g_0.xyz / _S97, direction_2) + _S96.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S96.sh_0.sh_b_0.xyz / _S97, direction_2) + _S96.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S96.weight_1) , _S50);
}


#line 1113
float3 probe_environment_0(float3 world_position_5, float3 normal_4, float3 direction_3, KernelContext_0 thread* kernelContext_14)
{

#line 1121
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_5, kernelContext_14->camera_0->probe_level_origin_0[int(0)].xyz, kernelContext_14->camera_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_14->camera_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_14->camera_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1123
    float3 _S98 = probe_level_environment_0(level_4, world_position_5, normal_4, direction_3, kernelContext_14);


    if(share_0 >= 1.0f)
    {

#line 1127
        return _S98;
    }

#line 1127
    float3 _S99 = probe_level_environment_0(level_4 + 1U, world_position_5, normal_4, direction_3, kernelContext_14);

    return _S99 * float3((1.0f - share_0))  + _S98 * float3(share_0) ;
}


#line 747
float2 decode_fixed_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 759
float2 fixed_pair_at_0(texture2d<float, access::sample> table_0, float2 at_1)
{
    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (table_0).get_width(0)),(*((&height_1)) = (table_0).get_height(0));
    float2 extent_5 = float2(float(width_1), float(height_1));
    float2 scaled_1 = saturate(at_1) * extent_5 - float2(0.5f) ;

#line 765
    float2 _S100 = float2(1.0f) ;
    float2 _S101 = extent_5 - _S100;

#line 766
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S101);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S102 = int2(low_1);
    int2 _S103 = int2(min(low_1 + _S100, _S101));
    int _S104 = _S102.x;

#line 772
    int _S105 = _S102.y;

#line 772
    int3 _S106 = int3(_S104, _S105, int(0));
    int _S107 = _S103.x;

#line 773
    int3 _S108 = int3(_S107, _S105, int(0));
    float2 _S109 = float2(weight_2.x) ;
    int _S110 = _S103.y;

#line 775
    int3 _S111 = int3(_S104, _S110, int(0));
    int3 _S112 = int3(_S107, _S110, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S108)).xy), uint(((_S108)).z)))), _S109), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S111)).xy), uint(((_S111)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S112)).xy), uint(((_S112)).z)))), _S109), float2(weight_2.y) );
}


float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_15)
{
    return fixed_pair_at_0(kernelContext_15->sky_prefilter_0, float2(up_0, roughness_1));
}


#line 805
float3 sky_prefiltered_0(float3 direction_4, float roughness_2, KernelContext_0 thread* kernelContext_16)
{
    float up_1 = clamp(direction_4.y, -1.0f, 1.0f);

#line 807
    float2 _S113 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_16);

    bool _S114 = up_1 >= 0.0f;

#line 809
    float3 far_0;

#line 809
    if(_S114)
    {

#line 809
        far_0 = kernelContext_16->camera_0->sky_0[int(0)].xyz;

#line 809
    }
    else
    {

#line 809
        far_0 = kernelContext_16->camera_0->sky_0[int(2)].xyz;

#line 809
    }

#line 809
    float3 opposite_0;
    if(_S114)
    {

#line 810
        opposite_0 = kernelContext_16->camera_0->sky_0[int(2)].xyz;

#line 810
    }
    else
    {

#line 810
        opposite_0 = kernelContext_16->camera_0->sky_0[int(0)].xyz;

#line 810
    }
    float _S115 = _S113.x;

#line 811
    float _S116 = _S113.y;
    return kernelContext_16->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S115 - _S116))  + far_0 * float3(_S115)  + opposite_0 * float3(_S116) ;
}


#line 788
float2 dfg_at_0(float n_dot_v_0, float roughness_3, KernelContext_0 thread* kernelContext_17)
{
    return fixed_pair_at_0(kernelContext_17->dfg_0, float2(n_dot_v_0, roughness_3));
}


#line 610
float2 pixel_of_0(float2 ndc_1, float2 size_1)
{
    return float2((ndc_1.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_1.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_2, float2 size_2)
{
    return float2(at_2.x / size_2.x * 2.0f - 1.0f, 1.0f - at_2.y / size_2.y * 2.0f);
}


#line 687
float cell_exit_0(float2 at_3, float2 forward_0, float size_3, float reach_2)
{

    float _S117 = forward_0.x;

#line 690
    bool _S118 = _S117 > 0.0f;

#line 690
    float along_x_0;

#line 690
    if(_S118)
    {

#line 690
        along_x_0 = (floor(at_3.x / size_3) + 1.0f) * size_3;

#line 690
    }
    else
    {

#line 690
        along_x_0 = floor(at_3.x / size_3) * size_3;

#line 690
    }
    float _S119 = forward_0.y;

#line 691
    bool _S120 = _S119 > 0.0f;

#line 691
    float along_y_0;

#line 691
    if(_S120)
    {

#line 691
        along_y_0 = (floor(at_3.y / size_3) + 1.0f) * size_3;

#line 691
    }
    else
    {

#line 691
        along_y_0 = floor(at_3.y / size_3) * size_3;

#line 691
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 692
    float _S121;

    if((abs(_S117)) < 9.99999997475242708e-07f)
    {

#line 694
        along_x_0 = reach_2;

#line 694
    }
    else
    {

#line 695
        if(_S118)
        {

#line 695
            _S121 = nudge_0;

#line 695
        }
        else
        {

#line 695
            _S121 = - nudge_0;

#line 695
        }

#line 695
        along_x_0 = (along_x_0 + _S121 - at_3.x) / _S117;

#line 694
    }


    if((abs(_S119)) < 9.99999997475242708e-07f)
    {

#line 697
        along_y_0 = reach_2;

#line 697
    }
    else
    {

#line 698
        if(_S120)
        {

#line 698
            _S121 = nudge_0;

#line 698
        }
        else
        {

#line 698
            _S121 = - nudge_0;

#line 698
        }

#line 698
        along_y_0 = (along_y_0 + _S121 - at_3.y) / _S119;

#line 697
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 646
float hiz_at_0(uint level_5, int2 texel_1, int2 extent_6, KernelContext_0 thread* kernelContext_18)
{
    int2 _S122 = int2(int(0), int(0));
    int3 at_4 = int3(clamp(texel_1, _S122, max(extent_6 - int2(int(1), int(1)), _S122)), int(0));
    switch(level_5)
    {
    case 0U:
        {

#line 653
            return ((kernelContext_18->scene_depth_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 1U:
        {

#line 655
            return ((kernelContext_18->hiz_1_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 2U:
        {

#line 657
            return ((kernelContext_18->hiz_2_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 3U:
        {

#line 659
            return ((kernelContext_18->hiz_3_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 4U:
        {

#line 661
            return ((kernelContext_18->hiz_4_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    default:
        {

#line 663
            return ((kernelContext_18->hiz_5_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    }

#line 663
}


#line 674
float view_z_of_0(float depth_5, KernelContext_0 thread* kernelContext_19)
{

#line 674
    float2 _S123 = unproject_z_1(depth_5, kernelContext_19);


    return _S123.x / _S123.y;
}


#line 629
float thickness_at_0(float advance_0, float depth_6)
{
    return max(advance_0, abs(depth_6) * 0.01999999955296516f);
}


#line 631
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 631
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 1144
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S124 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], texture2d<float, access::sample> dfg_1 [[texture(9)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 1144
    float3 reflection_0;

#line 1144
    thread KernelContext_0 kernelContext_20;

#line 1144
    (&kernelContext_20)->scene_depth_0 = scene_depth_1;

#line 1144
    (&kernelContext_20)->reflectivity_0 = reflectivity_1;

#line 1144
    (&kernelContext_20)->camera_0 = camera_1;

#line 1144
    (&kernelContext_20)->probes_0 = probes_1;

#line 1144
    (&kernelContext_20)->probe_visibility_0 = probe_visibility_1;

#line 1144
    (&kernelContext_20)->sky_prefilter_0 = sky_prefilter_1;

#line 1144
    (&kernelContext_20)->dfg_0 = dfg_1;

#line 1144
    (&kernelContext_20)->hiz_1_0 = hiz_1_1;

#line 1144
    (&kernelContext_20)->hiz_2_0 = hiz_2_1;

#line 1144
    (&kernelContext_20)->hiz_3_0 = hiz_3_1;

#line 1144
    (&kernelContext_20)->hiz_4_0 = hiz_4_1;

#line 1144
    (&kernelContext_20)->hiz_5_0 = hiz_5_1;

#line 1144
    (&kernelContext_20)->scene_color_0 = scene_color_1;

    thread uint width_2;
    thread uint height_2;



    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    int _S125 = int(width_2);

#line 1152
    int _S126 = int(height_2);

#line 1152
    int2 extent_7 = int2(_S125, _S126);
    float _S127 = float(width_2);

#line 1153
    float _S128 = float(height_2);

#line 1153
    float2 size_4 = float2(_S127, _S128);
    int2 _S129 = int2(position_0.xy);

#line 1161
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S130 = int3(_S129, int(0));

#line 1163
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S130)).xy), uint(((_S130)).z)));
    float _S131 = surface_0.w;

#line 1164
    float sharpness_0 = sharpness_of_0(_S131);

#line 1164
    float _S132 = depth_at_0(_S129, extent_7, &kernelContext_20);


    if(_S132 <= 0.0f)
    {

#line 1167
        pixelOutput_0 _S133 = { NOTHING_0 };

        return _S133;
    }

#line 1169
    float3 _S134 = view_position_0(_S129, _S132, size_4, &kernelContext_20);

#line 1169
    float3 _S135 = normal_at_0(_S129, _S134, extent_7, size_4, &kernelContext_20);

#line 1175
    float3 towards_0 = normalize(_S134);
    float3 ray_0 = reflect(towards_0, _S135);


    float4 _S136 = float4(ray_0, 0.0f);

#line 1179
    float3 reflection_direction_0 = normalize((((_S136) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 1179
    float3 _S137 = probe_environment_0((((float4(_S134, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, normalize((((float4(_S135, 0.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz), reflection_direction_0, &kernelContext_20);

#line 1179
    float3 _S138 = sky_prefiltered_0(reflection_direction_0, _S131, &kernelContext_20);

#line 1199
    float3 environment_0 = _S137 + _S138;

#line 1207
    float3 _S139 = - towards_0;
    float3 f0_0 = surface_0.xyz;

#line 1208
    float2 _S140 = dfg_at_0(saturate(dot(_S135, _S139)), _S131, &kernelContext_20);

    float3 env_brdf_0 = f0_0 * float3(_S140.x)  + float3(_S140.y) ;

#line 1215
    if(sharpness_0 <= 0.0f)
    {

#line 1215
        pixelOutput_0 _S141 = { float4(environment_0 * env_brdf_0, 0.0f) };

        return _S141;
    }


    float _S142 = saturate((1.0f - dot(ray_0, _S139)) / 0.05000000074505806f);


    float _S143 = _S134.z;

#line 1224
    float3 start_0 = _S134 + _S135 * float3((abs(_S143) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S136) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S144 = clip_start_0.w;

#line 1229
    if(_S144 <= 0.0f)
    {

#line 1229
        pixelOutput_0 _S145 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S145;
    }
    float2 _S146 = clip_start_0.xy;

#line 1233
    float2 _S147 = float2(_S144) ;

#line 1233
    float2 at_start_0 = pixel_of_0(_S146 / _S147, size_4);

#line 1239
    float2 _S148 = clip_ray_0.xy;

#line 1239
    float _S149 = clip_ray_0.w;

#line 1239
    float2 _S150 = float2(_S149) ;

#line 1239
    float2 ndc_rate_0 = (_S148 * _S147 - _S146 * _S150) / float2((_S144 * _S144)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S127, - ndc_rate_0.y * 0.5f * _S128);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 1242
        pixelOutput_0 _S151 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S151;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 1253
    float reach_3 = 0.75f * min(_S127, _S128);

    float _S152 = forward_1.x;

#line 1255
    float travel_0;

#line 1255
    if(_S152 > 0.0f)
    {

#line 1255
        travel_0 = min(reach_3, (_S127 - 1.0f - at_start_0.x) / _S152);

#line 1255
    }
    else
    {

        if(_S152 < 0.0f)
        {

#line 1259
            travel_0 = min(reach_3, - at_start_0.x / _S152);

#line 1259
        }
        else
        {

#line 1259
            travel_0 = reach_3;

#line 1259
        }

#line 1255
    }

#line 1263
    float _S153 = forward_1.y;

#line 1263
    if(_S153 > 0.0f)
    {

#line 1263
        travel_0 = min(travel_0, (_S128 - 1.0f - at_start_0.y) / _S153);

#line 1263
    }
    else
    {

        if(_S153 < 0.0f)
        {

#line 1267
            travel_0 = min(travel_0, - at_start_0.y / _S153);

#line 1267
        }

#line 1263
    }

#line 1275
    if(_S149 > 0.0f)
    {

#line 1275
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S148 / _S150, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 1275
    }
    else
    {

#line 1290
        if(_S149 < 0.0f)
        {

#line 1297
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_20)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 1302
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S144) / _S149)) ;

#line 1302
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 1290
        }

#line 1275
    }

#line 1309
    float _S154 = max(travel_0, 0.0f);
    if(_S154 <= 0.00390625f)
    {

#line 1310
        pixelOutput_0 _S155 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S155;
    }

#line 1319
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S154) , size_4);

#line 1319
    float when_end_0;

    if((abs(_S152)) >= (abs(_S153)))
    {

#line 1321
        float _S156 = ndc_end_0.x;

#line 1321
        when_end_0 = (_S156 * _S144 - clip_start_0.x) / (clip_ray_0.x - _S156 * _S149);

#line 1321
    }
    else
    {

#line 1322
        float _S157 = ndc_end_0.y;

#line 1322
        when_end_0 = (_S157 * _S144 - clip_start_0.y) / (clip_ray_0.y - _S157 * _S149);

#line 1321
    }

#line 1321
    bool _S158;

#line 1329
    if(!(when_end_0 > 0.0f))
    {

#line 1329
        _S158 = true;

#line 1329
    }
    else
    {

#line 1329
        _S158 = !isfinite(when_end_0);

#line 1329
    }

#line 1329
    if(_S158)
    {

#line 1329
        pixelOutput_0 _S159 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S159;
    }

#line 1337
    float inverse_w_start_0 = 1.0f / _S144;

    float inverse_w_end_0 = 1.0f / (_S144 + when_end_0 * _S149);
    float _S160 = start_0.z;

#line 1340
    float _S161 = _S160 * inverse_w_start_0;
    float _S162 = (_S160 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 1346
    float3 _S163 = environment_0 * env_brdf_0;
    uint _S164 = min((&kernelContext_20)->camera_0->hiz_0.x, 5U);

#line 1377
    float _S165 = _S160 - _S143;

#line 1377
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S154), _S154);

#line 1377
    float previous_gap_0 = _S165;

#line 1377
    float entry_z_0 = _S160;

#line 1377
    uint step_0 = 0U;

#line 1377
    uint level_6 = 0U;

    for(;;)
    {

#line 1379
        if(step_0 < 96U)
        {
        }
        else
        {

#line 1379
            reflection_0 = _S163;

#line 1379
            break;
        }
        float cell_2 = float(1U << level_6);
        float2 at_5 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S166 = min(at_travel_0 + cell_exit_0(at_5, forward_1, cell_2, _S154), _S154);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S166) ;
        float along_0 = _S166 / _S154;

        float exit_z_0 = mix(_S161, _S162, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 1387
        float _S167 = hiz_at_0(level_6, int2(floor(at_5 / float2(cell_2) )), int2(_S125 >> level_6, _S126 >> level_6), &kernelContext_20);

#line 1387
        float gap_0;

#line 1396
        if(_S167 <= 0.0f)
        {

#line 1396
            gap_0 = 1.0f;

#line 1396
        }
        else
        {

#line 1396
            float _S168 = view_z_of_0(_S167, &kernelContext_20);

#line 1396
            gap_0 = exit_z_0 - _S168;

#line 1396
        }

#line 1405
        bool _S169 = !(gap_0 > 0.0f);

#line 1405
        if(_S169)
        {

#line 1405
            _S158 = level_6 > 0U;

#line 1405
        }
        else
        {

#line 1405
            _S158 = false;

#line 1405
        }

#line 1405
        if(_S158)
        {

#line 1405
            level_6 = level_6 - 1U;

#line 1411
            step_0 = step_0 + 1U;

#line 1379
            continue;
        }

#line 1379
        bool _S170;

#line 1414
        if(_S169)
        {

#line 1414
            _S170 = previous_gap_0 > 0.0f;

#line 1414
        }
        else
        {

#line 1414
            _S170 = false;

#line 1414
        }

#line 1414
        if(_S170)
        {



            float behind_1 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_1 <= thickness_0)
            {

#line 1427
                float2 hit_at_0 = mix(at_5, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1442
                float confidence_0 = sharpness_0 * _S142 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S166 / reach_3) / 0.25f) * saturate(1.0f - behind_1 / thickness_0);
                int3 _S171 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_7 - int2(int(1), int(1))), int(0));

#line 1443
                reflection_0 = (((&kernelContext_20)->scene_color_0).read(vec<uint,2>(((_S171)).xy), uint(((_S171)).z))).xyz * env_brdf_0 * float3(confidence_0)  + _S163 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 1414
        }

#line 1455
        if(_S166 >= _S154)
        {

#line 1455
            reflection_0 = _S163;

            break;
        }



        uint _S172 = min(level_6 + 1U, _S164);

#line 1462
        at_travel_0 = _S166;

#line 1462
        previous_gap_0 = gap_0;

#line 1462
        entry_z_0 = exit_z_0;

#line 1462
        level_6 = _S172;

#line 1379
        step_0 = step_0 + 1U;

#line 1379
    }

#line 1379
    pixelOutput_0 _S173 = { float4(reflection_0, sharpness_0) };

#line 1470
    return _S173;
}


#line 1470
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 474
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 474
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(8)]], texture2d<float, access::sample> dfg_2 [[texture(9)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 474
    thread KernelContext_0 kernelContext_21;

#line 474
    (&kernelContext_21)->scene_depth_0 = scene_depth_2;

#line 474
    (&kernelContext_21)->reflectivity_0 = reflectivity_2;

#line 474
    (&kernelContext_21)->camera_0 = camera_2;

#line 474
    (&kernelContext_21)->probes_0 = probes_2;

#line 474
    (&kernelContext_21)->probe_visibility_0 = probe_visibility_2;

#line 474
    (&kernelContext_21)->sky_prefilter_0 = sky_prefilter_2;

#line 474
    (&kernelContext_21)->dfg_0 = dfg_2;

#line 474
    (&kernelContext_21)->hiz_1_0 = hiz_1_2;

#line 474
    (&kernelContext_21)->hiz_2_0 = hiz_2_2;

#line 474
    (&kernelContext_21)->hiz_3_0 = hiz_3_2;

#line 474
    (&kernelContext_21)->hiz_4_0 = hiz_4_2;

#line 474
    (&kernelContext_21)->hiz_5_0 = hiz_5_2;

#line 474
    (&kernelContext_21)->scene_color_0 = scene_color_2;

#line 1135
    thread FullscreenOutput_0 output_1;


    float2 _S174 = float2(float((index_3 << 1U) & 2U), float(index_3 & 2U));

#line 1138
    (&output_1)->uv_2 = _S174;
    (&output_1)->position_2 = float4(_S174 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 1139
    thread vertexMain_Result_0 _S175;

#line 1139
    (&_S175)->position_1 = output_1.position_2;

#line 1139
    (&_S175)->uv_1 = output_1.uv_2;

#line 1139
    return _S175;
}

