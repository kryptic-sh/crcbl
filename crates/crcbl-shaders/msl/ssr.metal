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


#line 974
float probe_level_reach_0(float3 world_position_0, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 974
    float reach_0 = 0.0f;

#line 974
    uint axis_0 = 0U;


    for(;;)
    {

#line 977
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 977
            break;
        }

#line 977
        uint _S19 = axis_0;

#line 977
        bool _S20;

        if((last_0[axis_0]) == 0.0f)
        {

#line 979
            _S20 = true;

#line 979
        }
        else
        {

#line 979
            _S20 = (inv_spacing_0[axis_0]) == 0.0f;

#line 979
        }

#line 979
        if(_S20)
        {

#line 980
            axis_0 = axis_0 + 1U;

#line 977
            continue;
        }

#line 977
        reach_0 = max(reach_0, abs(2.0f * ((world_position_0[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S19] - 1.0f));

#line 977
        axis_0 = axis_0 + 1U;

#line 977
    }

#line 984
    return reach_0;
}


#line 994
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 994
    uint level_0 = 0U;

    for(;;)
    {

#line 996
        uint _S21 = level_0 + 1U;

#line 996
        if(_S21 < levels_0)
        {
        }
        else
        {

#line 996
            break;
        }
        float _S22 = float(level_0);

#line 998
        float at_0 = reach_1 * exp2(- _S22);
        if(at_0 < 1.0f)
        {

#line 1000
            return float2(_S22, saturate((1.0f - at_0) / 0.25f));
        }

#line 996
        level_0 = _S21;

#line 996
    }

#line 1002
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 904
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


#line 882
float probe_weight_0(uint index_1, float3 probe_position_0, float3 world_position_1, float3 normal_0, KernelContext_0 thread* kernelContext_10)
{
    float3 to_probe_0 = probe_position_0 - (world_position_1 + normal_0 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 885
    float2 _S40 = probe_moments_0(index_1, - to_probe_0, kernelContext_10);

#line 891
    float _S41 = _S40.x;

#line 891
    float _S42 = max(_S40.y - _S41 * _S41, 0.0f);
    float behind_0 = to_surface_0 - _S41;
    float bound_0 = _S42 / (_S42 + behind_0 * behind_0);

#line 893
    float visible_0;
    if(to_surface_0 <= _S41)
    {

#line 894
        visible_0 = 1.0f;

#line 894
    }
    else
    {

#line 894
        visible_0 = bound_0 * bound_0 * bound_0;

#line 894
    }
    return max(visible_0, 0.00009999999747379f);
}


#line 152
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 914
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 941
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_1, float3 origin_1, float3 spacing_0, float3 world_position_2, float3 normal_1, KernelContext_0 thread* kernelContext_11)
{

#line 942
    uint _S43 = probe_row_0(level_2, cell_1, kernelContext_11);


    GpuProbe_natural_0 stored_0 = kernelContext_11->probes_0[_S43];

#line 945
    float _S44 = probe_weight_0(_S43, origin_1 + float3(cell_1) * spacing_0, world_position_2, normal_1, kernelContext_11);



    thread WeightedProbe_0 corner_0;

#line 949
    float4 _S45 = float4(_S44) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S45;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S45;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S45;
    (&corner_0)->weight_1 = _S44;
    return corner_0;
}


#line 925
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S46 = float4(t_0) ;

#line 928
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S46);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S46);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S46);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1039
float3 probe_level_environment_0(uint level_3, float3 world_position_3, float3 normal_2, float3 direction_2, KernelContext_0 thread* kernelContext_12)
{

#line 1039
    float3 _S47 = float3(1.0f) ;

    float3 _S48 = float3(0.0f, 0.0f, 0.0f);

#line 1041
    float3 last_1 = max(float3(kernelContext_12->camera_0->probe_counts_0.xyz) - _S47, _S48);



    float3 origin_2 = kernelContext_12->camera_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_12->camera_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_3 - origin_2) * inv_0, _S48, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S49 = uint3(base_0);
    uint3 _S50 = uint3(min(base_0 + _S47, last_1));

#line 1056
    float _S51 = inv_0.x;

#line 1056
    float _S52;

#line 1056
    if(_S51 != 0.0f)
    {

#line 1056
        _S52 = 1.0f / _S51;

#line 1056
    }
    else
    {

#line 1056
        _S52 = 0.0f;

#line 1056
    }
    float _S53 = inv_0.y;

#line 1057
    float _S54;

#line 1057
    if(_S53 != 0.0f)
    {

#line 1057
        _S54 = 1.0f / _S53;

#line 1057
    }
    else
    {

#line 1057
        _S54 = 0.0f;

#line 1057
    }
    float _S55 = inv_0.z;

#line 1058
    float _S56;

#line 1058
    if(_S55 != 0.0f)
    {

#line 1058
        _S56 = 1.0f / _S55;

#line 1058
    }
    else
    {

#line 1058
        _S56 = 0.0f;

#line 1058
    }

#line 1056
    float3 spacing_1 = float3(_S52, _S54, _S56);

#line 1065
    uint _S57 = _S49.x;

#line 1065
    uint _S58 = _S49.y;

#line 1065
    uint _S59 = _S49.z;

#line 1065
    WeightedProbe_0 _S60 = probe_corner_0(level_3, uint3(_S57, _S58, _S59), origin_2, spacing_1, world_position_3, normal_2, kernelContext_12);
    uint _S61 = _S50.x;

#line 1066
    WeightedProbe_0 _S62 = probe_corner_0(level_3, uint3(_S61, _S58, _S59), origin_2, spacing_1, world_position_3, normal_2, kernelContext_12);

#line 1066
    float _S63 = f_0.x;

#line 1066
    thread WeightedProbe_0 _S64 = _S60;

#line 1066
    thread WeightedProbe_0 _S65 = _S62;

#line 1066
    WeightedProbe_0 _S66 = lerp_probe_0(&_S64, &_S65, _S63);
    uint _S67 = _S50.y;

#line 1067
    WeightedProbe_0 _S68 = probe_corner_0(level_3, uint3(_S57, _S67, _S59), origin_2, spacing_1, world_position_3, normal_2, kernelContext_12);

#line 1067
    WeightedProbe_0 _S69 = probe_corner_0(level_3, uint3(_S61, _S67, _S59), origin_2, spacing_1, world_position_3, normal_2, kernelContext_12);

#line 1067
    thread WeightedProbe_0 _S70 = _S68;

#line 1067
    thread WeightedProbe_0 _S71 = _S69;

#line 1067
    WeightedProbe_0 _S72 = lerp_probe_0(&_S70, &_S71, _S63);

    uint _S73 = _S50.z;

#line 1069
    WeightedProbe_0 _S74 = probe_corner_0(level_3, uint3(_S57, _S58, _S73), origin_2, spacing_1, world_position_3, normal_2, kernelContext_12);

#line 1069
    WeightedProbe_0 _S75 = probe_corner_0(level_3, uint3(_S61, _S58, _S73), origin_2, spacing_1, world_position_3, normal_2, kernelContext_12);

#line 1069
    thread WeightedProbe_0 _S76 = _S74;

#line 1069
    thread WeightedProbe_0 _S77 = _S75;

#line 1069
    WeightedProbe_0 _S78 = lerp_probe_0(&_S76, &_S77, _S63);

#line 1069
    WeightedProbe_0 _S79 = probe_corner_0(level_3, uint3(_S57, _S67, _S73), origin_2, spacing_1, world_position_3, normal_2, kernelContext_12);

#line 1069
    WeightedProbe_0 _S80 = probe_corner_0(level_3, uint3(_S61, _S67, _S73), origin_2, spacing_1, world_position_3, normal_2, kernelContext_12);

#line 1069
    thread WeightedProbe_0 _S81 = _S79;

#line 1069
    thread WeightedProbe_0 _S82 = _S80;

#line 1069
    WeightedProbe_0 _S83 = lerp_probe_0(&_S81, &_S82, _S63);



    float _S84 = f_0.y;

#line 1073
    thread WeightedProbe_0 _S85 = _S66;

#line 1073
    thread WeightedProbe_0 _S86 = _S72;

#line 1073
    WeightedProbe_0 _S87 = lerp_probe_0(&_S85, &_S86, _S84);

#line 1073
    thread WeightedProbe_0 _S88 = _S78;

#line 1073
    thread WeightedProbe_0 _S89 = _S83;

#line 1073
    WeightedProbe_0 _S90 = lerp_probe_0(&_S88, &_S89, _S84);

    float _S91 = f_0.z;

#line 1075
    thread WeightedProbe_0 _S92 = _S87;

#line 1075
    thread WeightedProbe_0 _S93 = _S90;

#line 1075
    WeightedProbe_0 _S94 = lerp_probe_0(&_S92, &_S93, _S91);

#line 1075
    float3 _S95 = float3(2.09439516067504883f) ;

#line 1081
    return max(float3(dot(_S94.sh_0.sh_r_0.xyz / _S95, direction_2) + _S94.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S94.sh_0.sh_g_0.xyz / _S95, direction_2) + _S94.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S94.sh_0.sh_b_0.xyz / _S95, direction_2) + _S94.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S94.weight_1) , _S48);
}


#line 1098
float3 probe_environment_0(float3 world_position_4, float3 normal_3, float3 direction_3, KernelContext_0 thread* kernelContext_13)
{

#line 1106
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_4, kernelContext_13->camera_0->probe_level_origin_0[int(0)].xyz, kernelContext_13->camera_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_13->camera_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_13->camera_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1108
    float3 _S96 = probe_level_environment_0(level_4, world_position_4, normal_3, direction_3, kernelContext_13);


    if(share_0 >= 1.0f)
    {

#line 1112
        return _S96;
    }

#line 1112
    float3 _S97 = probe_level_environment_0(level_4 + 1U, world_position_4, normal_3, direction_3, kernelContext_13);

    return _S97 * float3((1.0f - share_0))  + _S96 * float3(share_0) ;
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
    float2 _S98 = float2(1.0f) ;
    float2 _S99 = extent_5 - _S98;

#line 766
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S99);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S100 = int2(low_1);
    int2 _S101 = int2(min(low_1 + _S98, _S99));
    int _S102 = _S100.x;

#line 772
    int _S103 = _S100.y;

#line 772
    int3 _S104 = int3(_S102, _S103, int(0));
    int _S105 = _S101.x;

#line 773
    int3 _S106 = int3(_S105, _S103, int(0));
    float2 _S107 = float2(weight_2.x) ;
    int _S108 = _S101.y;

#line 775
    int3 _S109 = int3(_S102, _S108, int(0));
    int3 _S110 = int3(_S105, _S108, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S104)).xy), uint(((_S104)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z)))), _S107), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S109)).xy), uint(((_S109)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S110)).xy), uint(((_S110)).z)))), _S107), float2(weight_2.y) );
}


float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_14)
{
    return fixed_pair_at_0(kernelContext_14->sky_prefilter_0, float2(up_0, roughness_1));
}


#line 805
float3 sky_prefiltered_0(float3 direction_4, float roughness_2, KernelContext_0 thread* kernelContext_15)
{
    float up_1 = clamp(direction_4.y, -1.0f, 1.0f);

#line 807
    float2 _S111 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_15);

    bool _S112 = up_1 >= 0.0f;

#line 809
    float3 far_0;

#line 809
    if(_S112)
    {

#line 809
        far_0 = kernelContext_15->camera_0->sky_0[int(0)].xyz;

#line 809
    }
    else
    {

#line 809
        far_0 = kernelContext_15->camera_0->sky_0[int(2)].xyz;

#line 809
    }

#line 809
    float3 opposite_0;
    if(_S112)
    {

#line 810
        opposite_0 = kernelContext_15->camera_0->sky_0[int(2)].xyz;

#line 810
    }
    else
    {

#line 810
        opposite_0 = kernelContext_15->camera_0->sky_0[int(0)].xyz;

#line 810
    }
    float _S113 = _S111.x;

#line 811
    float _S114 = _S111.y;
    return kernelContext_15->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S113 - _S114))  + far_0 * float3(_S113)  + opposite_0 * float3(_S114) ;
}


#line 788
float2 dfg_at_0(float n_dot_v_0, float roughness_3, KernelContext_0 thread* kernelContext_16)
{
    return fixed_pair_at_0(kernelContext_16->dfg_0, float2(n_dot_v_0, roughness_3));
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

    float _S115 = forward_0.x;

#line 690
    bool _S116 = _S115 > 0.0f;

#line 690
    float along_x_0;

#line 690
    if(_S116)
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
    float _S117 = forward_0.y;

#line 691
    bool _S118 = _S117 > 0.0f;

#line 691
    float along_y_0;

#line 691
    if(_S118)
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
    float _S119;

    if((abs(_S115)) < 9.99999997475242708e-07f)
    {

#line 694
        along_x_0 = reach_2;

#line 694
    }
    else
    {

#line 695
        if(_S116)
        {

#line 695
            _S119 = nudge_0;

#line 695
        }
        else
        {

#line 695
            _S119 = - nudge_0;

#line 695
        }

#line 695
        along_x_0 = (along_x_0 + _S119 - at_3.x) / _S115;

#line 694
    }


    if((abs(_S117)) < 9.99999997475242708e-07f)
    {

#line 697
        along_y_0 = reach_2;

#line 697
    }
    else
    {

#line 698
        if(_S118)
        {

#line 698
            _S119 = nudge_0;

#line 698
        }
        else
        {

#line 698
            _S119 = - nudge_0;

#line 698
        }

#line 698
        along_y_0 = (along_y_0 + _S119 - at_3.y) / _S117;

#line 697
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 646
float hiz_at_0(uint level_5, int2 texel_1, int2 extent_6, KernelContext_0 thread* kernelContext_17)
{
    int2 _S120 = int2(int(0), int(0));
    int3 at_4 = int3(clamp(texel_1, _S120, max(extent_6 - int2(int(1), int(1)), _S120)), int(0));
    switch(level_5)
    {
    case 0U:
        {

#line 653
            return ((kernelContext_17->scene_depth_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 1U:
        {

#line 655
            return ((kernelContext_17->hiz_1_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 2U:
        {

#line 657
            return ((kernelContext_17->hiz_2_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 3U:
        {

#line 659
            return ((kernelContext_17->hiz_3_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 4U:
        {

#line 661
            return ((kernelContext_17->hiz_4_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    default:
        {

#line 663
            return ((kernelContext_17->hiz_5_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    }

#line 663
}


#line 674
float view_z_of_0(float depth_5, KernelContext_0 thread* kernelContext_18)
{

#line 674
    float2 _S121 = unproject_z_1(depth_5, kernelContext_18);


    return _S121.x / _S121.y;
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


#line 1129
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S122 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], texture2d<float, access::sample> dfg_1 [[texture(9)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 1129
    float3 reflection_0;

#line 1129
    thread KernelContext_0 kernelContext_19;

#line 1129
    (&kernelContext_19)->scene_depth_0 = scene_depth_1;

#line 1129
    (&kernelContext_19)->reflectivity_0 = reflectivity_1;

#line 1129
    (&kernelContext_19)->camera_0 = camera_1;

#line 1129
    (&kernelContext_19)->probes_0 = probes_1;

#line 1129
    (&kernelContext_19)->probe_visibility_0 = probe_visibility_1;

#line 1129
    (&kernelContext_19)->sky_prefilter_0 = sky_prefilter_1;

#line 1129
    (&kernelContext_19)->dfg_0 = dfg_1;

#line 1129
    (&kernelContext_19)->hiz_1_0 = hiz_1_1;

#line 1129
    (&kernelContext_19)->hiz_2_0 = hiz_2_1;

#line 1129
    (&kernelContext_19)->hiz_3_0 = hiz_3_1;

#line 1129
    (&kernelContext_19)->hiz_4_0 = hiz_4_1;

#line 1129
    (&kernelContext_19)->hiz_5_0 = hiz_5_1;

#line 1129
    (&kernelContext_19)->scene_color_0 = scene_color_1;

    thread uint width_2;
    thread uint height_2;



    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    int _S123 = int(width_2);

#line 1137
    int _S124 = int(height_2);

#line 1137
    int2 extent_7 = int2(_S123, _S124);
    float _S125 = float(width_2);

#line 1138
    float _S126 = float(height_2);

#line 1138
    float2 size_4 = float2(_S125, _S126);
    int2 _S127 = int2(position_0.xy);

#line 1146
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S128 = int3(_S127, int(0));

#line 1148
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S128)).xy), uint(((_S128)).z)));
    float _S129 = surface_0.w;

#line 1149
    float sharpness_0 = sharpness_of_0(_S129);

#line 1149
    float _S130 = depth_at_0(_S127, extent_7, &kernelContext_19);


    if(_S130 <= 0.0f)
    {

#line 1152
        pixelOutput_0 _S131 = { NOTHING_0 };

        return _S131;
    }

#line 1154
    float3 _S132 = view_position_0(_S127, _S130, size_4, &kernelContext_19);

#line 1154
    float3 _S133 = normal_at_0(_S127, _S132, extent_7, size_4, &kernelContext_19);

#line 1160
    float3 towards_0 = normalize(_S132);
    float3 ray_0 = reflect(towards_0, _S133);


    float4 _S134 = float4(ray_0, 0.0f);

#line 1164
    float3 reflection_direction_0 = normalize((((_S134) * (matrix<float,int(4),int(4)> ((&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 1164
    float3 _S135 = probe_environment_0((((float4(_S132, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, normalize((((float4(_S133, 0.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_19)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz), reflection_direction_0, &kernelContext_19);

#line 1164
    float3 _S136 = sky_prefiltered_0(reflection_direction_0, _S129, &kernelContext_19);

#line 1184
    float3 environment_0 = _S135 + _S136;

#line 1192
    float3 _S137 = - towards_0;
    float3 f0_0 = surface_0.xyz;

#line 1193
    float2 _S138 = dfg_at_0(saturate(dot(_S133, _S137)), _S129, &kernelContext_19);

    float3 env_brdf_0 = f0_0 * float3(_S138.x)  + float3(_S138.y) ;

#line 1200
    if(sharpness_0 <= 0.0f)
    {

#line 1200
        pixelOutput_0 _S139 = { float4(environment_0 * env_brdf_0, 0.0f) };

        return _S139;
    }


    float _S140 = saturate((1.0f - dot(ray_0, _S137)) / 0.05000000074505806f);


    float _S141 = _S132.z;

#line 1209
    float3 start_0 = _S132 + _S133 * float3((abs(_S141) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_19)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_19)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_19)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_19)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_19)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_19)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_19)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_19)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_19)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_19)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_19)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_19)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_19)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_19)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_19)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_19)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S134) * (matrix<float,int(4),int(4)> ((&kernelContext_19)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_19)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_19)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_19)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_19)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_19)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_19)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_19)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_19)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_19)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_19)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_19)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_19)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_19)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_19)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_19)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S142 = clip_start_0.w;

#line 1214
    if(_S142 <= 0.0f)
    {

#line 1214
        pixelOutput_0 _S143 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S143;
    }
    float2 _S144 = clip_start_0.xy;

#line 1218
    float2 _S145 = float2(_S142) ;

#line 1218
    float2 at_start_0 = pixel_of_0(_S144 / _S145, size_4);

#line 1224
    float2 _S146 = clip_ray_0.xy;

#line 1224
    float _S147 = clip_ray_0.w;

#line 1224
    float2 _S148 = float2(_S147) ;

#line 1224
    float2 ndc_rate_0 = (_S146 * _S145 - _S144 * _S148) / float2((_S142 * _S142)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S125, - ndc_rate_0.y * 0.5f * _S126);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 1227
        pixelOutput_0 _S149 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S149;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 1238
    float reach_3 = 0.75f * min(_S125, _S126);

    float _S150 = forward_1.x;

#line 1240
    float travel_0;

#line 1240
    if(_S150 > 0.0f)
    {

#line 1240
        travel_0 = min(reach_3, (_S125 - 1.0f - at_start_0.x) / _S150);

#line 1240
    }
    else
    {

        if(_S150 < 0.0f)
        {

#line 1244
            travel_0 = min(reach_3, - at_start_0.x / _S150);

#line 1244
        }
        else
        {

#line 1244
            travel_0 = reach_3;

#line 1244
        }

#line 1240
    }

#line 1248
    float _S151 = forward_1.y;

#line 1248
    if(_S151 > 0.0f)
    {

#line 1248
        travel_0 = min(travel_0, (_S126 - 1.0f - at_start_0.y) / _S151);

#line 1248
    }
    else
    {

        if(_S151 < 0.0f)
        {

#line 1252
            travel_0 = min(travel_0, - at_start_0.y / _S151);

#line 1252
        }

#line 1248
    }

#line 1260
    if(_S147 > 0.0f)
    {

#line 1260
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S146 / _S148, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 1260
    }
    else
    {

#line 1275
        if(_S147 < 0.0f)
        {

#line 1282
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_19)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_19)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 1287
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S142) / _S147)) ;

#line 1287
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 1275
        }

#line 1260
    }

#line 1294
    float _S152 = max(travel_0, 0.0f);
    if(_S152 <= 0.00390625f)
    {

#line 1295
        pixelOutput_0 _S153 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S153;
    }

#line 1304
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S152) , size_4);

#line 1304
    float when_end_0;

    if((abs(_S150)) >= (abs(_S151)))
    {

#line 1306
        float _S154 = ndc_end_0.x;

#line 1306
        when_end_0 = (_S154 * _S142 - clip_start_0.x) / (clip_ray_0.x - _S154 * _S147);

#line 1306
    }
    else
    {

#line 1307
        float _S155 = ndc_end_0.y;

#line 1307
        when_end_0 = (_S155 * _S142 - clip_start_0.y) / (clip_ray_0.y - _S155 * _S147);

#line 1306
    }

#line 1306
    bool _S156;

#line 1314
    if(!(when_end_0 > 0.0f))
    {

#line 1314
        _S156 = true;

#line 1314
    }
    else
    {

#line 1314
        _S156 = !isfinite(when_end_0);

#line 1314
    }

#line 1314
    if(_S156)
    {

#line 1314
        pixelOutput_0 _S157 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S157;
    }

#line 1322
    float inverse_w_start_0 = 1.0f / _S142;

    float inverse_w_end_0 = 1.0f / (_S142 + when_end_0 * _S147);
    float _S158 = start_0.z;

#line 1325
    float _S159 = _S158 * inverse_w_start_0;
    float _S160 = (_S158 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 1331
    float3 _S161 = environment_0 * env_brdf_0;
    uint _S162 = min((&kernelContext_19)->camera_0->hiz_0.x, 5U);

#line 1362
    float _S163 = _S158 - _S141;

#line 1362
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S152), _S152);

#line 1362
    float previous_gap_0 = _S163;

#line 1362
    float entry_z_0 = _S158;

#line 1362
    uint step_0 = 0U;

#line 1362
    uint level_6 = 0U;

    for(;;)
    {

#line 1364
        if(step_0 < 96U)
        {
        }
        else
        {

#line 1364
            reflection_0 = _S161;

#line 1364
            break;
        }
        float cell_2 = float(1U << level_6);
        float2 at_5 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S164 = min(at_travel_0 + cell_exit_0(at_5, forward_1, cell_2, _S152), _S152);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S164) ;
        float along_0 = _S164 / _S152;

        float exit_z_0 = mix(_S159, _S160, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 1372
        float _S165 = hiz_at_0(level_6, int2(floor(at_5 / float2(cell_2) )), int2(_S123 >> level_6, _S124 >> level_6), &kernelContext_19);

#line 1372
        float gap_0;

#line 1381
        if(_S165 <= 0.0f)
        {

#line 1381
            gap_0 = 1.0f;

#line 1381
        }
        else
        {

#line 1381
            float _S166 = view_z_of_0(_S165, &kernelContext_19);

#line 1381
            gap_0 = exit_z_0 - _S166;

#line 1381
        }

#line 1390
        bool _S167 = !(gap_0 > 0.0f);

#line 1390
        if(_S167)
        {

#line 1390
            _S156 = level_6 > 0U;

#line 1390
        }
        else
        {

#line 1390
            _S156 = false;

#line 1390
        }

#line 1390
        if(_S156)
        {

#line 1390
            level_6 = level_6 - 1U;

#line 1396
            step_0 = step_0 + 1U;

#line 1364
            continue;
        }

#line 1364
        bool _S168;

#line 1399
        if(_S167)
        {

#line 1399
            _S168 = previous_gap_0 > 0.0f;

#line 1399
        }
        else
        {

#line 1399
            _S168 = false;

#line 1399
        }

#line 1399
        if(_S168)
        {



            float behind_1 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_1 <= thickness_0)
            {

#line 1412
                float2 hit_at_0 = mix(at_5, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1427
                float confidence_0 = sharpness_0 * _S140 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S164 / reach_3) / 0.25f) * saturate(1.0f - behind_1 / thickness_0);
                int3 _S169 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_7 - int2(int(1), int(1))), int(0));

#line 1428
                reflection_0 = (((&kernelContext_19)->scene_color_0).read(vec<uint,2>(((_S169)).xy), uint(((_S169)).z))).xyz * env_brdf_0 * float3(confidence_0)  + _S161 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 1399
        }

#line 1440
        if(_S164 >= _S152)
        {

#line 1440
            reflection_0 = _S161;

            break;
        }



        uint _S170 = min(level_6 + 1U, _S162);

#line 1447
        at_travel_0 = _S164;

#line 1447
        previous_gap_0 = gap_0;

#line 1447
        entry_z_0 = exit_z_0;

#line 1447
        level_6 = _S170;

#line 1364
        step_0 = step_0 + 1U;

#line 1364
    }

#line 1364
    pixelOutput_0 _S171 = { float4(reflection_0, sharpness_0) };

#line 1455
    return _S171;
}


#line 1455
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
[[vertex]] vertexMain_Result_0 vertexMain(uint index_2 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(8)]], texture2d<float, access::sample> dfg_2 [[texture(9)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 474
    thread KernelContext_0 kernelContext_20;

#line 474
    (&kernelContext_20)->scene_depth_0 = scene_depth_2;

#line 474
    (&kernelContext_20)->reflectivity_0 = reflectivity_2;

#line 474
    (&kernelContext_20)->camera_0 = camera_2;

#line 474
    (&kernelContext_20)->probes_0 = probes_2;

#line 474
    (&kernelContext_20)->probe_visibility_0 = probe_visibility_2;

#line 474
    (&kernelContext_20)->sky_prefilter_0 = sky_prefilter_2;

#line 474
    (&kernelContext_20)->dfg_0 = dfg_2;

#line 474
    (&kernelContext_20)->hiz_1_0 = hiz_1_2;

#line 474
    (&kernelContext_20)->hiz_2_0 = hiz_2_2;

#line 474
    (&kernelContext_20)->hiz_3_0 = hiz_3_2;

#line 474
    (&kernelContext_20)->hiz_4_0 = hiz_4_2;

#line 474
    (&kernelContext_20)->hiz_5_0 = hiz_5_2;

#line 474
    (&kernelContext_20)->scene_color_0 = scene_color_2;

#line 1120
    thread FullscreenOutput_0 output_1;


    float2 _S172 = float2(float((index_2 << 1U) & 2U), float(index_2 & 2U));

#line 1123
    (&output_1)->uv_2 = _S172;
    (&output_1)->position_2 = float4(_S172 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 1124
    thread vertexMain_Result_0 _S173;

#line 1124
    (&_S173)->position_1 = output_1.position_2;

#line 1124
    (&_S173)->uv_1 = output_1.uv_2;

#line 1124
    return _S173;
}

