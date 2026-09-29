#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 403 "shaders/ssr.slang"
float sharpness_of_0(float roughness_0)
{
    return saturate(1.0f - roughness_0 / 0.5f);
}


#line 188
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 116
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
    float4 atmosphere_0;
};


#line 231
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 214
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    texture2d<float, access::sample> reflectivity_0;
    SsrParams_natural_0 constant* camera_0;
    GpuProbe_natural_0 device* probes_0;
    texture2d_array<float, access::sample> probe_visibility_0;
    texture2d<float, access::sample> sky_prefilter_0;
    packed_float4 device* sky_view_0;
    texture2d<float, access::sample> dfg_0;
    depth2d<float, access::sample> hiz_1_0;
    depth2d<float, access::sample> hiz_2_0;
    depth2d<float, access::sample> hiz_3_0;
    depth2d<float, access::sample> hiz_4_0;
    depth2d<float, access::sample> hiz_5_0;
    texture2d<float, access::sample> scene_color_0;
};


#line 549
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 552
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 549
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 552
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 570
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 570
float2 unproject_z_1(float depth_1, KernelContext_0 thread* kernelContext_3)
{
    return float2((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].z * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].w * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 601
float4 unproject_0(float2 ndc_0, float depth_2, KernelContext_0 thread* kernelContext_4)
{

#line 601
    float2 _S3 = unproject_z_0(depth_2, kernelContext_4);


    return float4((&kernelContext_4->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].y, _S3.x, _S3.y);
}


#line 617
float3 view_position_0(int2 pixel_2, float depth_3, float2 extent_2, KernelContext_0 thread* kernelContext_5)
{

#line 617
    float4 _S4 = unproject_0(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_3, kernelContext_5);

#line 628
    return _S4.xyz / float3(_S4.w) ;
}


#line 617
float3 view_position_1(int2 pixel_3, float depth_4, float2 extent_3, KernelContext_0 thread* kernelContext_6)
{

#line 617
    float4 _S5 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_4, kernelContext_6);

#line 628
    return _S5.xyz / float3(_S5.w) ;
}


#line 643
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_7)
{
    int2 _S6 = pixel_4 + int2(int(-1), int(0));

#line 645
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_7);

#line 645
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_7);
    int2 _S9 = pixel_4 + int2(int(1), int(0));

#line 646
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_7);

#line 646
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_7);
    int2 _S12 = pixel_4 + int2(int(0), int(-1));

#line 647
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_7);

#line 647
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_7);
    int2 _S15 = pixel_4 + int2(int(0), int(1));

#line 648
    float _S16 = depth_at_1(_S15, extent_4, kernelContext_7);

#line 648
    float3 _S17 = view_position_1(_S15, _S16, size_0, kernelContext_7);

    float _S18 = centre_0.z;

#line 650
    float3 horizontal_0;
    if((abs(_S11.z - _S18)) < (abs(_S18 - _S8.z)))
    {

#line 651
        horizontal_0 = _S11 - centre_0;

#line 651
    }
    else
    {

#line 651
        horizontal_0 = centre_0 - _S8;

#line 651
    }

#line 651
    float3 vertical_0;


    if((abs(_S17.z - _S18)) < (abs(_S18 - _S14.z)))
    {

#line 654
        vertical_0 = _S17 - centre_0;

#line 654
    }
    else
    {

#line 654
        vertical_0 = centre_0 - _S14;

#line 654
    }

#line 664
    return normalize(cross(vertical_0, horizontal_0));
}


#line 1184
float probe_level_reach_0(float3 world_position_0, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 1184
    float reach_0 = 0.0f;

#line 1184
    uint axis_0 = 0U;


    for(;;)
    {

#line 1187
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 1187
            break;
        }

#line 1187
        uint _S19 = axis_0;

#line 1187
        bool _S20;

        if((last_0[axis_0]) == 0.0f)
        {

#line 1189
            _S20 = true;

#line 1189
        }
        else
        {

#line 1189
            _S20 = (inv_spacing_0[axis_0]) == 0.0f;

#line 1189
        }

#line 1189
        if(_S20)
        {

#line 1190
            axis_0 = axis_0 + 1U;

#line 1187
            continue;
        }

#line 1187
        reach_0 = max(reach_0, abs(2.0f * ((world_position_0[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S19] - 1.0f));

#line 1187
        axis_0 = axis_0 + 1U;

#line 1187
    }

#line 1194
    return reach_0;
}


#line 1204
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 1204
    uint level_0 = 0U;

    for(;;)
    {

#line 1206
        uint _S21 = level_0 + 1U;

#line 1206
        if(_S21 < levels_0)
        {
        }
        else
        {

#line 1206
            break;
        }
        float _S22 = float(level_0);

#line 1208
        float at_0 = reach_1 * exp2(- _S22);
        if(at_0 < 1.0f)
        {

#line 1210
            return float2(_S22, saturate((1.0f - at_0) / 0.25f));
        }

#line 1206
        level_0 = _S21;

#line 1206
    }

#line 1212
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 1093
uint probe_wrap_0(uint cell_0, uint offset_0, uint count_0)
{
    uint at_1 = cell_0 + offset_0;

#line 1095
    uint _S23;
    if(at_1 >= count_0)
    {

#line 1096
        _S23 = at_1 - count_0;

#line 1096
    }
    else
    {

#line 1096
        _S23 = at_1;

#line 1096
    }

#line 1096
    return _S23;
}


#line 1109
uint probe_row_0(uint level_1, uint3 cell_1, KernelContext_0 thread* kernelContext_8)
{
    uint3 counts_0 = kernelContext_8->camera_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_8->camera_0->probe_level_offset_0[level_1].xyz;
    uint _S24 = counts_0.x;
    uint _S25 = counts_0.y;



    return min(kernelContext_8->camera_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_1.z, offset_1.z, counts_0.z) * _S25 + probe_wrap_0(cell_1.y, offset_1.y, _S25)) * _S24 + probe_wrap_0(cell_1.x, offset_1.x, _S24), max(kernelContext_8->camera_0->probe_counts_0.w, 1U) - 1U);
}


#line 995
float sign_not_zero_0(float value_0)
{

#line 995
    float _S26;

    if(value_0 >= 0.0f)
    {

#line 997
        _S26 = 1.0f;

#line 997
    }
    else
    {

#line 997
        _S26 = -1.0f;

#line 997
    }

#line 997
    return _S26;
}


#line 1005
float2 oct_encode_0(float3 direction_0)
{
    float _S27 = direction_0.y;
    float2 p_0 = direction_0.xz / float2(max(abs(direction_0.x) + abs(_S27) + abs(direction_0.z), 9.99999968265522539e-21f)) ;

#line 1008
    float2 p_1;
    if(_S27 < 0.0f)
    {
        float _S28 = p_0.y;

#line 1011
        float _S29 = p_0.x;

#line 1011
        p_1 = float2((1.0f - abs(_S28)) * sign_not_zero_0(_S29), (1.0f - abs(_S29)) * sign_not_zero_0(_S28));

#line 1009
    }
    else
    {

#line 1009
        p_1 = p_0;

#line 1009
    }

#line 1014
    return p_1;
}


#line 1023
float2 probe_moments_0(uint index_0, float3 direction_1, KernelContext_0 thread* kernelContext_9)
{

#line 1023
    texture2d_array<float, access::sample> _S30 = kernelContext_9->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S30).get_width(0)),(*((&height_0)) = (_S30).get_height(0)),(*((&layers_0)) = (_S30).get_array_size());

#line 1028
    float2 _S31 = float2(0.5f) ;

#line 1028
    float2 _S32 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_1) * _S31 + _S31) * float2(16.0f)  + _S32 - _S31;
    float2 _S33 = float2(float(width_0), float(height_0)) - _S32;

#line 1032
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S33);
    float2 high_0 = min(low_0 + _S32, _S33);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_0, max(layers_0, 1U) - 1U));

    int _S34 = int(low_0.x);

#line 1037
    int _S35 = int(low_0.y);

#line 1037
    int4 _S36 = int4(_S34, _S35, layer_0, int(0));
    int _S37 = int(high_0.x);

#line 1038
    int4 _S38 = int4(_S37, _S35, layer_0, int(0));
    int _S39 = int(high_0.y);

#line 1039
    int4 _S40 = int4(_S34, _S39, layer_0, int(0));
    int4 _S41 = int4(_S37, _S39, layer_0, int(0));
    float2 _S42 = float2(weight_0.x) ;

#line 1041
    return mix(mix(((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S36)).xy), uint(((_S36)).z), uint(((_S36)).w))).xy, ((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S38)).xy), uint(((_S38)).z), uint(((_S38)).w))).xy, _S42), mix(((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S40)).xy), uint(((_S40)).z), uint(((_S40)).w))).xy, ((kernelContext_9->probe_visibility_0).read(vec<uint,2>(((_S41)).xy), uint(((_S41)).z), uint(((_S41)).w))).xy, _S42), float2(weight_0.y) );
}


#line 1056
float probe_chebyshev_0(uint index_1, float3 probe_position_0, float3 world_position_1, float3 normal_0, KernelContext_0 thread* kernelContext_10)
{
    float3 to_probe_0 = probe_position_0 - (world_position_1 + normal_0 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 1059
    float2 _S43 = probe_moments_0(index_1, - to_probe_0, kernelContext_10);

#line 1065
    float _S44 = _S43.x;

#line 1065
    float _S45 = max(_S43.y - _S44 * _S44, 0.0f);
    float behind_0 = to_surface_0 - _S44;
    float bound_0 = _S45 / (_S45 + behind_0 * behind_0);

#line 1067
    float _S46;
    if(to_surface_0 <= _S44)
    {

#line 1068
        _S46 = 1.0f;

#line 1068
    }
    else
    {

#line 1068
        _S46 = bound_0 * bound_0 * bound_0;

#line 1068
    }

#line 1068
    return _S46;
}


#line 1084
float probe_weight_0(uint index_2, float3 probe_position_1, float3 world_position_2, float3 normal_1, KernelContext_0 thread* kernelContext_11)
{

#line 1084
    float _S47 = probe_chebyshev_0(index_2, probe_position_1, world_position_2, normal_1, kernelContext_11);

    return max(_S47, 0.00009999999747379f);
}


#line 180
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 1124
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 1151
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_2, float3 origin_1, float3 spacing_0, float3 world_position_3, float3 normal_2, KernelContext_0 thread* kernelContext_12)
{

#line 1152
    uint _S48 = probe_row_0(level_2, cell_2, kernelContext_12);


    GpuProbe_natural_0 stored_0 = kernelContext_12->probes_0[_S48];

#line 1155
    float _S49 = probe_weight_0(_S48, origin_1 + float3(cell_2) * spacing_0, world_position_3, normal_2, kernelContext_12);



    thread WeightedProbe_0 corner_0;

#line 1159
    float4 _S50 = float4(_S49) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S50;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S50;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S50;
    (&corner_0)->weight_1 = _S49;
    return corner_0;
}


#line 1135
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S51 = float4(t_0) ;

#line 1138
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S51);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S51);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S51);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1249
float3 probe_level_environment_0(uint level_3, float3 world_position_4, float3 normal_3, float3 direction_2, KernelContext_0 thread* kernelContext_13)
{

#line 1249
    float3 _S52 = float3(1.0f) ;

    float3 _S53 = float3(0.0f, 0.0f, 0.0f);

#line 1251
    float3 last_1 = max(float3(kernelContext_13->camera_0->probe_counts_0.xyz) - _S52, _S53);



    float3 origin_2 = kernelContext_13->camera_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_13->camera_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_4 - origin_2) * inv_0, _S53, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S54 = uint3(base_0);
    uint3 _S55 = uint3(min(base_0 + _S52, last_1));

#line 1266
    float _S56 = inv_0.x;

#line 1266
    float _S57;

#line 1266
    if(_S56 != 0.0f)
    {

#line 1266
        _S57 = 1.0f / _S56;

#line 1266
    }
    else
    {

#line 1266
        _S57 = 0.0f;

#line 1266
    }
    float _S58 = inv_0.y;

#line 1267
    float _S59;

#line 1267
    if(_S58 != 0.0f)
    {

#line 1267
        _S59 = 1.0f / _S58;

#line 1267
    }
    else
    {

#line 1267
        _S59 = 0.0f;

#line 1267
    }
    float _S60 = inv_0.z;

#line 1268
    float _S61;

#line 1268
    if(_S60 != 0.0f)
    {

#line 1268
        _S61 = 1.0f / _S60;

#line 1268
    }
    else
    {

#line 1268
        _S61 = 0.0f;

#line 1268
    }

#line 1266
    float3 spacing_1 = float3(_S57, _S59, _S61);

#line 1275
    uint _S62 = _S54.x;

#line 1275
    uint _S63 = _S54.y;

#line 1275
    uint _S64 = _S54.z;

#line 1275
    WeightedProbe_0 _S65 = probe_corner_0(level_3, uint3(_S62, _S63, _S64), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);
    uint _S66 = _S55.x;

#line 1276
    WeightedProbe_0 _S67 = probe_corner_0(level_3, uint3(_S66, _S63, _S64), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1276
    float _S68 = f_0.x;

#line 1276
    thread WeightedProbe_0 _S69 = _S65;

#line 1276
    thread WeightedProbe_0 _S70 = _S67;

#line 1276
    WeightedProbe_0 _S71 = lerp_probe_0(&_S69, &_S70, _S68);
    uint _S72 = _S55.y;

#line 1277
    WeightedProbe_0 _S73 = probe_corner_0(level_3, uint3(_S62, _S72, _S64), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1277
    WeightedProbe_0 _S74 = probe_corner_0(level_3, uint3(_S66, _S72, _S64), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1277
    thread WeightedProbe_0 _S75 = _S73;

#line 1277
    thread WeightedProbe_0 _S76 = _S74;

#line 1277
    WeightedProbe_0 _S77 = lerp_probe_0(&_S75, &_S76, _S68);

    uint _S78 = _S55.z;

#line 1279
    WeightedProbe_0 _S79 = probe_corner_0(level_3, uint3(_S62, _S63, _S78), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1279
    WeightedProbe_0 _S80 = probe_corner_0(level_3, uint3(_S66, _S63, _S78), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1279
    thread WeightedProbe_0 _S81 = _S79;

#line 1279
    thread WeightedProbe_0 _S82 = _S80;

#line 1279
    WeightedProbe_0 _S83 = lerp_probe_0(&_S81, &_S82, _S68);

#line 1279
    WeightedProbe_0 _S84 = probe_corner_0(level_3, uint3(_S62, _S72, _S78), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1279
    WeightedProbe_0 _S85 = probe_corner_0(level_3, uint3(_S66, _S72, _S78), origin_2, spacing_1, world_position_4, normal_3, kernelContext_13);

#line 1279
    thread WeightedProbe_0 _S86 = _S84;

#line 1279
    thread WeightedProbe_0 _S87 = _S85;

#line 1279
    WeightedProbe_0 _S88 = lerp_probe_0(&_S86, &_S87, _S68);



    float _S89 = f_0.y;

#line 1283
    thread WeightedProbe_0 _S90 = _S71;

#line 1283
    thread WeightedProbe_0 _S91 = _S77;

#line 1283
    WeightedProbe_0 _S92 = lerp_probe_0(&_S90, &_S91, _S89);

#line 1283
    thread WeightedProbe_0 _S93 = _S83;

#line 1283
    thread WeightedProbe_0 _S94 = _S88;

#line 1283
    WeightedProbe_0 _S95 = lerp_probe_0(&_S93, &_S94, _S89);

    float _S96 = f_0.z;

#line 1285
    thread WeightedProbe_0 _S97 = _S92;

#line 1285
    thread WeightedProbe_0 _S98 = _S95;

#line 1285
    WeightedProbe_0 _S99 = lerp_probe_0(&_S97, &_S98, _S96);

#line 1285
    float3 _S100 = float3(2.09439516067504883f) ;

#line 1291
    return max(float3(dot(_S99.sh_0.sh_r_0.xyz / _S100, direction_2) + _S99.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S99.sh_0.sh_g_0.xyz / _S100, direction_2) + _S99.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S99.sh_0.sh_b_0.xyz / _S100, direction_2) + _S99.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S99.weight_1) , _S53);
}


#line 1308
float3 probe_environment_0(float3 world_position_5, float3 normal_4, float3 direction_3, KernelContext_0 thread* kernelContext_14)
{

#line 1316
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_5, kernelContext_14->camera_0->probe_level_origin_0[int(0)].xyz, kernelContext_14->camera_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_14->camera_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_14->camera_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1318
    float3 _S101 = probe_level_environment_0(level_4, world_position_5, normal_4, direction_3, kernelContext_14);


    if(share_0 >= 1.0f)
    {

#line 1322
        return _S101;
    }

#line 1322
    float3 _S102 = probe_level_environment_0(level_4 + 1U, world_position_5, normal_4, direction_3, kernelContext_14);

    return _S102 * float3((1.0f - share_0))  + _S101 * float3(share_0) ;
}


#line 829
float2 decode_fixed_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 841
float2 fixed_pair_at_0(texture2d<float, access::sample> table_0, float2 at_2)
{
    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (table_0).get_width(0)),(*((&height_1)) = (table_0).get_height(0));
    float2 extent_5 = float2(float(width_1), float(height_1));
    float2 scaled_1 = saturate(at_2) * extent_5 - float2(0.5f) ;

#line 847
    float2 _S103 = float2(1.0f) ;
    float2 _S104 = extent_5 - _S103;

#line 848
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S104);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S105 = int2(low_1);
    int2 _S106 = int2(min(low_1 + _S103, _S104));
    int _S107 = _S105.x;

#line 854
    int _S108 = _S105.y;

#line 854
    int3 _S109 = int3(_S107, _S108, int(0));
    int _S110 = _S106.x;

#line 855
    int3 _S111 = int3(_S110, _S108, int(0));
    float2 _S112 = float2(weight_2.x) ;
    int _S113 = _S106.y;

#line 857
    int3 _S114 = int3(_S107, _S113, int(0));
    int3 _S115 = int3(_S110, _S113, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S109)).xy), uint(((_S109)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S111)).xy), uint(((_S111)).z)))), _S112), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S114)).xy), uint(((_S114)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S115)).xy), uint(((_S115)).z)))), _S112), float2(weight_2.y) );
}


float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_15)
{
    return fixed_pair_at_0(kernelContext_15->sky_prefilter_0, float2(up_0, roughness_1));
}


#line 887
float3 sky_prefiltered_0(float3 direction_4, float roughness_2, KernelContext_0 thread* kernelContext_16)
{
    float up_1 = clamp(direction_4.y, -1.0f, 1.0f);

#line 889
    float2 _S116 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_16);

    bool _S117 = up_1 >= 0.0f;

#line 891
    float3 far_0;

#line 891
    if(_S117)
    {

#line 891
        far_0 = kernelContext_16->camera_0->sky_0[int(0)].xyz;

#line 891
    }
    else
    {

#line 891
        far_0 = kernelContext_16->camera_0->sky_0[int(2)].xyz;

#line 891
    }

#line 891
    float3 opposite_0;
    if(_S117)
    {

#line 892
        opposite_0 = kernelContext_16->camera_0->sky_0[int(2)].xyz;

#line 892
    }
    else
    {

#line 892
        opposite_0 = kernelContext_16->camera_0->sky_0[int(0)].xyz;

#line 892
    }
    float _S118 = _S116.x;

#line 893
    float _S119 = _S116.y;
    return kernelContext_16->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S118 - _S119))  + far_0 * float3(_S118)  + opposite_0 * float3(_S119) ;
}


#line 906
float3 sky_view_at_0(float up_2, float azimuth_cosine_0, KernelContext_0 thread* kernelContext_17)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_0 = clamp(up_2, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_0));

#line 910
    float _S120;
    if(clamped_0 >= 0.0f)
    {

#line 911
        _S120 = root_0;

#line 911
    }
    else
    {

#line 911
        _S120 = - root_0;

#line 911
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    float x0_0 = clamp(floor(across_0), 0.0f, 95.0f);

    float fx_0 = clamp(across_0 - x0_0, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S120, 0.0f, 1.0f) * 64.0f - 0.5f;
    float y0_0 = clamp(floor(down_0), 0.0f, 63.0f);

    float fy_0 = clamp(down_0 - y0_0, 0.0f, 1.0f);

    uint row0_0 = uint(y0_0) * 96U;
    uint row1_0 = uint(min(y0_0 + 1.0f, 63.0f)) * 96U;
    uint _S121 = uint(x0_0);

#line 925
    float3 _S122 = float3((1.0f - fx_0)) ;
    uint _S123 = uint(min(x0_0 + 1.0f, 95.0f));

#line 926
    float3 _S124 = float3(fx_0) ;


    return ((float4(*(kernelContext_17->sky_view_0+(row0_0 + _S121))) ).xyz * _S122 + (float4(*(kernelContext_17->sky_view_0+(row0_0 + _S123))) ).xyz * _S124) * float3((1.0f - fy_0))  + ((float4(*(kernelContext_17->sky_view_0+(row1_0 + _S121))) ).xyz * _S122 + (float4(*(kernelContext_17->sky_view_0+(row1_0 + _S123))) ).xyz * _S124) * float3(fy_0) ;
}


#line 939
float3 atmosphere_radiance_0(float3 direction_5, KernelContext_0 thread* kernelContext_18)
{
    float3 sun_0 = kernelContext_18->camera_0->atmosphere_0.xyz;
    float _S125 = direction_5.x;

#line 942
    float _S126 = direction_5.z;

#line 942
    float view_flat_0 = sqrt(_S125 * _S125 + _S126 * _S126);
    float _S127 = sun_0.x;

#line 943
    float _S128 = sun_0.z;

#line 943
    float sun_flat_0 = sqrt(_S127 * _S127 + _S128 * _S128);

#line 943
    bool _S129;

    if(view_flat_0 > 0.0f)
    {

#line 945
        _S129 = sun_flat_0 > 0.0f;

#line 945
    }
    else
    {

#line 945
        _S129 = false;

#line 945
    }

#line 945
    float cosine_0;

#line 945
    if(_S129)
    {

#line 945
        cosine_0 = (_S125 * _S127 + _S126 * _S128) / (view_flat_0 * sun_flat_0);

#line 945
    }
    else
    {

#line 945
        cosine_0 = 1.0f;

#line 945
    }

#line 945
    float3 _S130 = sky_view_at_0(direction_5.y, cosine_0, kernelContext_18);



    return _S130;
}


#line 979
float3 sky_environment_0(float3 direction_6, float roughness_3, float share_1, KernelContext_0 thread* kernelContext_19)
{

#line 979
    float3 _S131 = sky_prefiltered_0(direction_6, roughness_3, kernelContext_19);


    if((kernelContext_19->camera_0->atmosphere_0.w) <= 0.0f)
    {
        return _S131;
    }



    float3 _S132 = _S131 * float3((1.0f - share_1)) ;

#line 989
    float3 _S133 = atmosphere_radiance_0(direction_6, kernelContext_19);

#line 989
    return _S132 + _S133 * float3(share_1) ;
}


#line 870
float2 dfg_at_0(float n_dot_v_0, float roughness_4, KernelContext_0 thread* kernelContext_20)
{
    return fixed_pair_at_0(kernelContext_20->dfg_0, float2(n_dot_v_0, roughness_4));
}


#line 673
float2 pixel_of_0(float2 ndc_1, float2 size_1)
{
    return float2((ndc_1.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_1.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_3, float2 size_2)
{
    return float2(at_3.x / size_2.x * 2.0f - 1.0f, 1.0f - at_3.y / size_2.y * 2.0f);
}


#line 769
float cell_exit_0(float2 at_4, float2 forward_0, float size_3, float reach_2)
{

    float _S134 = forward_0.x;

#line 772
    bool _S135 = _S134 > 0.0f;

#line 772
    float along_x_0;

#line 772
    if(_S135)
    {

#line 772
        along_x_0 = (floor(at_4.x / size_3) + 1.0f) * size_3;

#line 772
    }
    else
    {

#line 772
        along_x_0 = floor(at_4.x / size_3) * size_3;

#line 772
    }
    float _S136 = forward_0.y;

#line 773
    bool _S137 = _S136 > 0.0f;

#line 773
    float along_y_0;

#line 773
    if(_S137)
    {

#line 773
        along_y_0 = (floor(at_4.y / size_3) + 1.0f) * size_3;

#line 773
    }
    else
    {

#line 773
        along_y_0 = floor(at_4.y / size_3) * size_3;

#line 773
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 774
    float _S138;

    if((abs(_S134)) < 9.99999997475242708e-07f)
    {

#line 776
        along_x_0 = reach_2;

#line 776
    }
    else
    {

#line 777
        if(_S135)
        {

#line 777
            _S138 = nudge_0;

#line 777
        }
        else
        {

#line 777
            _S138 = - nudge_0;

#line 777
        }

#line 777
        along_x_0 = (along_x_0 + _S138 - at_4.x) / _S134;

#line 776
    }


    if((abs(_S136)) < 9.99999997475242708e-07f)
    {

#line 779
        along_y_0 = reach_2;

#line 779
    }
    else
    {

#line 780
        if(_S137)
        {

#line 780
            _S138 = nudge_0;

#line 780
        }
        else
        {

#line 780
            _S138 = - nudge_0;

#line 780
        }

#line 780
        along_y_0 = (along_y_0 + _S138 - at_4.y) / _S136;

#line 779
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 702
int2 hiz_clamp_0(int2 texel_1, int2 extent_6, uint level_5)
{
    return min(texel_1, max(int2((extent_6.x) >> level_5, (extent_6.y) >> level_5) - int2(int(1), int(1)), int2(int(0), int(0))));
}


#line 731
float hiz_at_0(uint level_6, int2 texel_2, int2 extent_7, KernelContext_0 thread* kernelContext_21)
{
    int2 _S139 = int2(int(0), int(0));
    int2 clamped_1 = clamp(texel_2, _S139, max(int2((extent_7.x) >> level_6, (extent_7.y) >> level_6) - int2(int(1), int(1)), _S139));
    int3 _S140 = int3(hiz_clamp_0(clamped_1, extent_7, 0U), int(0));

#line 735
    float d0_0 = ((kernelContext_21->scene_depth_0).read(vec<uint,2>(((_S140)).xy), uint(((_S140)).z)));
    int3 _S141 = int3(hiz_clamp_0(clamped_1, extent_7, 1U), int(0));

#line 736
    float d1_0 = ((kernelContext_21->hiz_1_0).read(vec<uint,2>(((_S141)).xy), uint(((_S141)).z)));
    int3 _S142 = int3(hiz_clamp_0(clamped_1, extent_7, 2U), int(0));

#line 737
    float d2_0 = ((kernelContext_21->hiz_2_0).read(vec<uint,2>(((_S142)).xy), uint(((_S142)).z)));
    int3 _S143 = int3(hiz_clamp_0(clamped_1, extent_7, 3U), int(0));

#line 738
    float d3_0 = ((kernelContext_21->hiz_3_0).read(vec<uint,2>(((_S143)).xy), uint(((_S143)).z)));
    int3 _S144 = int3(hiz_clamp_0(clamped_1, extent_7, 4U), int(0));

#line 739
    float d4_0 = ((kernelContext_21->hiz_4_0).read(vec<uint,2>(((_S144)).xy), uint(((_S144)).z)));
    int3 _S145 = int3(hiz_clamp_0(clamped_1, extent_7, 5U), int(0));

#line 740
    float d5_0 = ((kernelContext_21->hiz_5_0).read(vec<uint,2>(((_S145)).xy), uint(((_S145)).z)));

#line 740
    float _S146;
    if(level_6 == 0U)
    {

#line 741
        _S146 = d0_0;

#line 741
    }
    else
    {

#line 742
        if(level_6 == 1U)
        {

#line 742
            _S146 = d1_0;

#line 742
        }
        else
        {

#line 743
            if(level_6 == 2U)
            {

#line 743
                _S146 = d2_0;

#line 743
            }
            else
            {

#line 744
                if(level_6 == 3U)
                {

#line 744
                    _S146 = d3_0;

#line 744
                }
                else
                {

#line 745
                    if(level_6 == 4U)
                    {

#line 745
                        _S146 = d4_0;

#line 745
                    }
                    else
                    {

#line 745
                        _S146 = d5_0;

#line 745
                    }

#line 744
                }

#line 743
            }

#line 742
        }

#line 741
    }

#line 741
    return _S146;
}


#line 756
float view_z_of_0(float depth_5, KernelContext_0 thread* kernelContext_22)
{

#line 756
    float2 _S147 = unproject_z_1(depth_5, kernelContext_22);


    return _S147.x / _S147.y;
}


#line 692
float thickness_at_0(float advance_0, float depth_6)
{
    return max(advance_0, abs(depth_6) * 0.01999999955296516f);
}


#line 694
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 694
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 1339
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S148 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], packed_float4 device* sky_view_1 [[buffer(2)]], texture2d<float, access::sample> dfg_1 [[texture(9)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 1339
    float3 reflection_0;

#line 1339
    thread KernelContext_0 kernelContext_23;

#line 1339
    (&kernelContext_23)->scene_depth_0 = scene_depth_1;

#line 1339
    (&kernelContext_23)->reflectivity_0 = reflectivity_1;

#line 1339
    (&kernelContext_23)->camera_0 = camera_1;

#line 1339
    (&kernelContext_23)->probes_0 = probes_1;

#line 1339
    (&kernelContext_23)->probe_visibility_0 = probe_visibility_1;

#line 1339
    (&kernelContext_23)->sky_prefilter_0 = sky_prefilter_1;

#line 1339
    (&kernelContext_23)->sky_view_0 = sky_view_1;

#line 1339
    (&kernelContext_23)->dfg_0 = dfg_1;

#line 1339
    (&kernelContext_23)->hiz_1_0 = hiz_1_1;

#line 1339
    (&kernelContext_23)->hiz_2_0 = hiz_2_1;

#line 1339
    (&kernelContext_23)->hiz_3_0 = hiz_3_1;

#line 1339
    (&kernelContext_23)->hiz_4_0 = hiz_4_1;

#line 1339
    (&kernelContext_23)->hiz_5_0 = hiz_5_1;

#line 1339
    (&kernelContext_23)->scene_color_0 = scene_color_1;

    thread uint width_2;
    thread uint height_2;



    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    int2 extent_8 = int2(int(width_2), int(height_2));
    float _S149 = float(width_2);

#line 1348
    float _S150 = float(height_2);

#line 1348
    float2 size_4 = float2(_S149, _S150);
    int2 _S151 = int2(position_0.xy);

#line 1356
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S152 = int3(_S151, int(0));

#line 1358
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S152)).xy), uint(((_S152)).z)));
    float _S153 = surface_0.w;

#line 1359
    float sharpness_0 = sharpness_of_0(_S153);

#line 1359
    float _S154 = depth_at_0(_S151, extent_8, &kernelContext_23);


    if(_S154 <= 0.0f)
    {

#line 1362
        pixelOutput_0 _S155 = { NOTHING_0 };

        return _S155;
    }

#line 1364
    float3 _S156 = view_position_0(_S151, _S154, size_4, &kernelContext_23);

#line 1364
    float3 _S157 = normal_at_0(_S151, _S156, extent_8, size_4, &kernelContext_23);

#line 1370
    float3 towards_0 = normalize(_S156);
    float3 ray_0 = reflect(towards_0, _S157);


    float4 _S158 = float4(ray_0, 0.0f);

#line 1374
    float3 reflection_direction_0 = normalize((((_S158) * (matrix<float,int(4),int(4)> ((&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 1374
    float3 _S159 = probe_environment_0((((float4(_S156, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, normalize((((float4(_S157, 0.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_23)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz), reflection_direction_0, &kernelContext_23);

#line 1374
    float3 _S160 = sky_environment_0(reflection_direction_0, _S153, sharpness_0, &kernelContext_23);

#line 1397
    float3 environment_0 = _S159 + _S160;

#line 1405
    float3 _S161 = - towards_0;
    float3 f0_0 = surface_0.xyz;

#line 1406
    float2 _S162 = dfg_at_0(saturate(dot(_S157, _S161)), _S153, &kernelContext_23);

    float3 env_brdf_0 = f0_0 * float3(_S162.x)  + float3(_S162.y) ;

#line 1413
    if(sharpness_0 <= 0.0f)
    {

#line 1413
        pixelOutput_0 _S163 = { float4(environment_0 * env_brdf_0, 0.0f) };

        return _S163;
    }


    float _S164 = saturate((1.0f - dot(ray_0, _S161)) / 0.05000000074505806f);


    float _S165 = _S156.z;

#line 1422
    float3 start_0 = _S156 + _S157 * float3((abs(_S165) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_23)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_23)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_23)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_23)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_23)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_23)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_23)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_23)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_23)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_23)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_23)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_23)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_23)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_23)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_23)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S158) * (matrix<float,int(4),int(4)> ((&kernelContext_23)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_23)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_23)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_23)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_23)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_23)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_23)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_23)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_23)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_23)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_23)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_23)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_23)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_23)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_23)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_23)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S166 = clip_start_0.w;

#line 1427
    if(_S166 <= 0.0f)
    {

#line 1427
        pixelOutput_0 _S167 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S167;
    }
    float2 _S168 = clip_start_0.xy;

#line 1431
    float2 _S169 = float2(_S166) ;

#line 1431
    float2 at_start_0 = pixel_of_0(_S168 / _S169, size_4);

#line 1437
    float2 _S170 = clip_ray_0.xy;

#line 1437
    float _S171 = clip_ray_0.w;

#line 1437
    float2 _S172 = float2(_S171) ;

#line 1437
    float2 ndc_rate_0 = (_S170 * _S169 - _S168 * _S172) / float2((_S166 * _S166)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S149, - ndc_rate_0.y * 0.5f * _S150);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 1440
        pixelOutput_0 _S173 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S173;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 1451
    float reach_3 = 0.75f * min(_S149, _S150);

    float _S174 = forward_1.x;

#line 1453
    float travel_0;

#line 1453
    if(_S174 > 0.0f)
    {

#line 1453
        travel_0 = min(reach_3, (_S149 - 1.0f - at_start_0.x) / _S174);

#line 1453
    }
    else
    {

        if(_S174 < 0.0f)
        {

#line 1457
            travel_0 = min(reach_3, - at_start_0.x / _S174);

#line 1457
        }
        else
        {

#line 1457
            travel_0 = reach_3;

#line 1457
        }

#line 1453
    }

#line 1461
    float _S175 = forward_1.y;

#line 1461
    if(_S175 > 0.0f)
    {

#line 1461
        travel_0 = min(travel_0, (_S150 - 1.0f - at_start_0.y) / _S175);

#line 1461
    }
    else
    {

        if(_S175 < 0.0f)
        {

#line 1465
            travel_0 = min(travel_0, - at_start_0.y / _S175);

#line 1465
        }

#line 1461
    }

#line 1473
    if(_S171 > 0.0f)
    {

#line 1473
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S170 / _S172, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 1473
    }
    else
    {

#line 1488
        if(_S171 < 0.0f)
        {

#line 1495
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_23)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 1500
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S166) / _S171)) ;

#line 1500
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 1488
        }

#line 1473
    }

#line 1507
    float _S176 = max(travel_0, 0.0f);
    if(_S176 <= 0.00390625f)
    {

#line 1508
        pixelOutput_0 _S177 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S177;
    }

#line 1517
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S176) , size_4);

#line 1517
    float when_end_0;

    if((abs(_S174)) >= (abs(_S175)))
    {

#line 1519
        float _S178 = ndc_end_0.x;

#line 1519
        when_end_0 = (_S178 * _S166 - clip_start_0.x) / (clip_ray_0.x - _S178 * _S171);

#line 1519
    }
    else
    {

#line 1520
        float _S179 = ndc_end_0.y;

#line 1520
        when_end_0 = (_S179 * _S166 - clip_start_0.y) / (clip_ray_0.y - _S179 * _S171);

#line 1519
    }

#line 1519
    bool _S180;

#line 1527
    if(!(when_end_0 > 0.0f))
    {

#line 1527
        _S180 = true;

#line 1527
    }
    else
    {

#line 1527
        _S180 = !isfinite(when_end_0);

#line 1527
    }

#line 1527
    if(_S180)
    {

#line 1527
        pixelOutput_0 _S181 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S181;
    }

#line 1535
    float inverse_w_start_0 = 1.0f / _S166;

    float inverse_w_end_0 = 1.0f / (_S166 + when_end_0 * _S171);
    float _S182 = start_0.z;

#line 1538
    float _S183 = _S182 * inverse_w_start_0;
    float _S184 = (_S182 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 1544
    float3 _S185 = environment_0 * env_brdf_0;
    uint _S186 = min((&kernelContext_23)->camera_0->hiz_0.x, 5U);

#line 1575
    float _S187 = _S182 - _S165;

#line 1575
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S176), _S176);

#line 1575
    float previous_gap_0 = _S187;

#line 1575
    float entry_z_0 = _S182;

#line 1575
    uint step_0 = 0U;

#line 1575
    uint level_7 = 0U;

    for(;;)
    {

#line 1577
        if(step_0 < 96U)
        {
        }
        else
        {

#line 1577
            reflection_0 = _S185;

#line 1577
            break;
        }
        float cell_3 = float(1U << level_7);
        float2 at_5 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S188 = min(at_travel_0 + cell_exit_0(at_5, forward_1, cell_3, _S176), _S176);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S188) ;
        float along_0 = _S188 / _S176;

        float exit_z_0 = mix(_S183, _S184, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 1585
        float _S189 = hiz_at_0(level_7, int2(floor(at_5 / float2(cell_3) )), extent_8, &kernelContext_23);

#line 1585
        float gap_0;

#line 1592
        if(_S189 <= 0.0f)
        {

#line 1592
            gap_0 = 1.0f;

#line 1592
        }
        else
        {

#line 1592
            float _S190 = view_z_of_0(_S189, &kernelContext_23);

#line 1592
            gap_0 = exit_z_0 - _S190;

#line 1592
        }

#line 1601
        bool _S191 = !(gap_0 > 0.0f);

#line 1601
        if(_S191)
        {

#line 1601
            _S180 = level_7 > 0U;

#line 1601
        }
        else
        {

#line 1601
            _S180 = false;

#line 1601
        }

#line 1601
        if(_S180)
        {

#line 1601
            level_7 = level_7 - 1U;

#line 1607
            step_0 = step_0 + 1U;

#line 1577
            continue;
        }

#line 1577
        bool _S192;

#line 1610
        if(_S191)
        {

#line 1610
            _S192 = previous_gap_0 > 0.0f;

#line 1610
        }
        else
        {

#line 1610
            _S192 = false;

#line 1610
        }

#line 1610
        if(_S192)
        {



            float behind_1 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_1 <= thickness_0)
            {

#line 1623
                float2 hit_at_0 = mix(at_5, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1638
                float confidence_0 = sharpness_0 * _S164 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S188 / reach_3) / 0.25f) * saturate(1.0f - behind_1 / thickness_0);
                int3 _S193 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_8 - int2(int(1), int(1))), int(0));

#line 1639
                reflection_0 = (((&kernelContext_23)->scene_color_0).read(vec<uint,2>(((_S193)).xy), uint(((_S193)).z))).xyz * env_brdf_0 * float3(confidence_0)  + _S185 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 1610
        }

#line 1651
        if(_S188 >= _S176)
        {

#line 1651
            reflection_0 = _S185;

            break;
        }



        uint _S194 = min(level_7 + 1U, _S186);

#line 1658
        at_travel_0 = _S188;

#line 1658
        previous_gap_0 = gap_0;

#line 1658
        entry_z_0 = exit_z_0;

#line 1658
        level_7 = _S194;

#line 1577
        step_0 = step_0 + 1U;

#line 1577
    }

#line 1577
    pixelOutput_0 _S195 = { float4(reflection_0, sharpness_0) };

#line 1666
    return _S195;
}


#line 1666
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 537
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 537
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(8)]], packed_float4 device* sky_view_2 [[buffer(2)]], texture2d<float, access::sample> dfg_2 [[texture(9)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 537
    thread KernelContext_0 kernelContext_24;

#line 537
    (&kernelContext_24)->scene_depth_0 = scene_depth_2;

#line 537
    (&kernelContext_24)->reflectivity_0 = reflectivity_2;

#line 537
    (&kernelContext_24)->camera_0 = camera_2;

#line 537
    (&kernelContext_24)->probes_0 = probes_2;

#line 537
    (&kernelContext_24)->probe_visibility_0 = probe_visibility_2;

#line 537
    (&kernelContext_24)->sky_prefilter_0 = sky_prefilter_2;

#line 537
    (&kernelContext_24)->sky_view_0 = sky_view_2;

#line 537
    (&kernelContext_24)->dfg_0 = dfg_2;

#line 537
    (&kernelContext_24)->hiz_1_0 = hiz_1_2;

#line 537
    (&kernelContext_24)->hiz_2_0 = hiz_2_2;

#line 537
    (&kernelContext_24)->hiz_3_0 = hiz_3_2;

#line 537
    (&kernelContext_24)->hiz_4_0 = hiz_4_2;

#line 537
    (&kernelContext_24)->hiz_5_0 = hiz_5_2;

#line 537
    (&kernelContext_24)->scene_color_0 = scene_color_2;

#line 1330
    thread FullscreenOutput_0 output_1;


    float2 _S196 = float2(float((index_3 << 1U) & 2U), float(index_3 & 2U));

#line 1333
    (&output_1)->uv_2 = _S196;
    (&output_1)->position_2 = float4(_S196 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 1334
    thread vertexMain_Result_0 _S197;

#line 1334
    (&_S197)->position_1 = output_1.position_2;

#line 1334
    (&_S197)->uv_1 = output_1.uv_2;

#line 1334
    return _S197;
}

