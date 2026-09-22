#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 116 "shaders/ssr.slang"
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
    packed_float4 device* sky_view_0;
    texture2d<float, access::sample> dfg_0;
    depth2d<float, access::sample> hiz_1_0;
    depth2d<float, access::sample> hiz_2_0;
    depth2d<float, access::sample> hiz_3_0;
    depth2d<float, access::sample> hiz_4_0;
    depth2d<float, access::sample> hiz_5_0;
    texture2d<float, access::sample> scene_color_0;
};


#line 552 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 555
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 552
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 555
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 563
int2 representative_pixel_0(int2 block_0, int2 extent_2, KernelContext_0 thread* kernelContext_2)
{
    int2 first_0 = block_0 * int2(int(2)) ;
    int2 _S3 = int2(int(0), int(0));

#line 566
    int2 _S4 = int2(int(1), int(1));

#line 566
    int2 _S5 = extent_2 - _S4;

#line 566
    int2 best_0 = clamp(first_0, _S3, _S5);

#line 566
    float _S6 = depth_at_1(best_0, extent_2, kernelContext_2);

    int2 candidate_0 = clamp(first_0 + int2(int(1), int(0)), _S3, _S5);

#line 568
    float _S7 = depth_at_1(candidate_0, extent_2, kernelContext_2);

#line 568
    float best_depth_0;

#line 568
    int2 best_1;

    if(_S7 > _S6)
    {

#line 570
        best_depth_0 = _S7;

#line 570
        best_1 = candidate_0;

#line 570
    }
    else
    {

#line 570
        best_depth_0 = _S6;

#line 570
        best_1 = best_0;

#line 570
    }

#line 575
    int2 candidate_1 = clamp(first_0 + int2(int(0), int(1)), _S3, _S5);

#line 575
    float _S8 = depth_at_1(candidate_1, extent_2, kernelContext_2);

    if(_S8 > best_depth_0)
    {

#line 577
        best_depth_0 = _S8;

#line 577
        best_1 = candidate_1;

#line 577
    }

#line 582
    int2 candidate_2 = clamp(first_0 + _S4, _S3, _S5);

#line 582
    float _S9 = depth_at_1(candidate_2, extent_2, kernelContext_2);

    if(_S9 > best_depth_0)
    {

#line 584
        best_1 = candidate_2;

#line 584
    }



    return best_1;
}


#line 406
float sharpness_of_0(float roughness_0)
{
    return saturate(1.0f - roughness_0 / 0.5f);
}


#line 606
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_3)
{
    return float2((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 606
float2 unproject_z_1(float depth_1, KernelContext_0 thread* kernelContext_4)
{
    return float2((&kernelContext_4->camera_0->inv_proj_0)->data_0[int(2)].z * depth_1 + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(2)].w * depth_1 + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 637
float4 unproject_0(float2 ndc_0, float depth_2, KernelContext_0 thread* kernelContext_5)
{

#line 637
    float2 _S10 = unproject_z_0(depth_2, kernelContext_5);


    return float4((&kernelContext_5->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_5->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_5->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_5->camera_0->inv_proj_0)->data_0[int(3)].y, _S10.x, _S10.y);
}


#line 653
float3 view_position_0(int2 pixel_2, float depth_3, float2 extent_3, KernelContext_0 thread* kernelContext_6)
{

#line 653
    float4 _S11 = unproject_0(float2((float(pixel_2.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_3.y * 2.0f), depth_3, kernelContext_6);

#line 664
    return _S11.xyz / float3(_S11.w) ;
}


#line 653
float3 view_position_1(int2 pixel_3, float depth_4, float2 extent_4, KernelContext_0 thread* kernelContext_7)
{

#line 653
    float4 _S12 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_4.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_4.y * 2.0f), depth_4, kernelContext_7);

#line 664
    return _S12.xyz / float3(_S12.w) ;
}


#line 679
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_5, float2 size_0, KernelContext_0 thread* kernelContext_8)
{
    int2 _S13 = pixel_4 + int2(int(-1), int(0));

#line 681
    float _S14 = depth_at_1(_S13, extent_5, kernelContext_8);

#line 681
    float3 _S15 = view_position_1(_S13, _S14, size_0, kernelContext_8);
    int2 _S16 = pixel_4 + int2(int(1), int(0));

#line 682
    float _S17 = depth_at_1(_S16, extent_5, kernelContext_8);

#line 682
    float3 _S18 = view_position_1(_S16, _S17, size_0, kernelContext_8);
    int2 _S19 = pixel_4 + int2(int(0), int(-1));

#line 683
    float _S20 = depth_at_1(_S19, extent_5, kernelContext_8);

#line 683
    float3 _S21 = view_position_1(_S19, _S20, size_0, kernelContext_8);
    int2 _S22 = pixel_4 + int2(int(0), int(1));

#line 684
    float _S23 = depth_at_1(_S22, extent_5, kernelContext_8);

#line 684
    float3 _S24 = view_position_1(_S22, _S23, size_0, kernelContext_8);

    float _S25 = centre_0.z;

#line 686
    float3 horizontal_0;
    if((abs(_S18.z - _S25)) < (abs(_S25 - _S15.z)))
    {

#line 687
        horizontal_0 = _S18 - centre_0;

#line 687
    }
    else
    {

#line 687
        horizontal_0 = centre_0 - _S15;

#line 687
    }

#line 687
    float3 vertical_0;


    if((abs(_S24.z - _S25)) < (abs(_S25 - _S21.z)))
    {

#line 690
        vertical_0 = _S24 - centre_0;

#line 690
    }
    else
    {

#line 690
        vertical_0 = centre_0 - _S21;

#line 690
    }

#line 700
    return normalize(cross(vertical_0, horizontal_0));
}


#line 1201
float probe_level_reach_0(float3 world_position_0, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 1201
    float reach_0 = 0.0f;

#line 1201
    uint axis_0 = 0U;


    for(;;)
    {

#line 1204
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 1204
            break;
        }

#line 1204
        uint _S26 = axis_0;

#line 1204
        bool _S27;

        if((last_0[axis_0]) == 0.0f)
        {

#line 1206
            _S27 = true;

#line 1206
        }
        else
        {

#line 1206
            _S27 = (inv_spacing_0[axis_0]) == 0.0f;

#line 1206
        }

#line 1206
        if(_S27)
        {

#line 1207
            axis_0 = axis_0 + 1U;

#line 1204
            continue;
        }

#line 1204
        reach_0 = max(reach_0, abs(2.0f * ((world_position_0[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S26] - 1.0f));

#line 1204
        axis_0 = axis_0 + 1U;

#line 1204
    }

#line 1211
    return reach_0;
}


#line 1221
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 1221
    uint level_0 = 0U;

    for(;;)
    {

#line 1223
        uint _S28 = level_0 + 1U;

#line 1223
        if(_S28 < levels_0)
        {
        }
        else
        {

#line 1223
            break;
        }
        float _S29 = float(level_0);

#line 1225
        float at_0 = reach_1 * exp2(- _S29);
        if(at_0 < 1.0f)
        {

#line 1227
            return float2(_S29, saturate((1.0f - at_0) / 0.25f));
        }

#line 1223
        level_0 = _S28;

#line 1223
    }

#line 1229
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 1110
uint probe_wrap_0(uint cell_0, uint offset_0, uint count_0)
{
    uint at_1 = cell_0 + offset_0;

#line 1112
    uint _S30;
    if(at_1 >= count_0)
    {

#line 1113
        _S30 = at_1 - count_0;

#line 1113
    }
    else
    {

#line 1113
        _S30 = at_1;

#line 1113
    }

#line 1113
    return _S30;
}


#line 1126
uint probe_row_0(uint level_1, uint3 cell_1, KernelContext_0 thread* kernelContext_9)
{
    uint3 counts_0 = kernelContext_9->camera_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_9->camera_0->probe_level_offset_0[level_1].xyz;
    uint _S31 = counts_0.x;
    uint _S32 = counts_0.y;



    return min(kernelContext_9->camera_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_1.z, offset_1.z, counts_0.z) * _S32 + probe_wrap_0(cell_1.y, offset_1.y, _S32)) * _S31 + probe_wrap_0(cell_1.x, offset_1.x, _S31), max(kernelContext_9->camera_0->probe_counts_0.w, 1U) - 1U);
}


#line 1012
float sign_not_zero_0(float value_0)
{

#line 1012
    float _S33;

    if(value_0 >= 0.0f)
    {

#line 1014
        _S33 = 1.0f;

#line 1014
    }
    else
    {

#line 1014
        _S33 = -1.0f;

#line 1014
    }

#line 1014
    return _S33;
}


#line 1022
float2 oct_encode_0(float3 direction_0)
{
    float _S34 = direction_0.y;
    float2 p_0 = direction_0.xz / float2(max(abs(direction_0.x) + abs(_S34) + abs(direction_0.z), 9.99999968265522539e-21f)) ;

#line 1025
    float2 p_1;
    if(_S34 < 0.0f)
    {
        float _S35 = p_0.y;

#line 1028
        float _S36 = p_0.x;

#line 1028
        p_1 = float2((1.0f - abs(_S35)) * sign_not_zero_0(_S36), (1.0f - abs(_S36)) * sign_not_zero_0(_S35));

#line 1026
    }
    else
    {

#line 1026
        p_1 = p_0;

#line 1026
    }

#line 1031
    return p_1;
}


#line 1040
float2 probe_moments_0(uint index_0, float3 direction_1, KernelContext_0 thread* kernelContext_10)
{

#line 1040
    texture2d_array<float, access::sample> _S37 = kernelContext_10->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S37).get_width(0)),(*((&height_0)) = (_S37).get_height(0)),(*((&layers_0)) = (_S37).get_array_size());

#line 1045
    float2 _S38 = float2(0.5f) ;

#line 1045
    float2 _S39 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_1) * _S38 + _S38) * float2(16.0f)  + _S39 - _S38;
    float2 _S40 = float2(float(width_0), float(height_0)) - _S39;

#line 1049
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S40);
    float2 high_0 = min(low_0 + _S39, _S40);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_0, max(layers_0, 1U) - 1U));

    int _S41 = int(low_0.x);

#line 1054
    int _S42 = int(low_0.y);

#line 1054
    int4 _S43 = int4(_S41, _S42, layer_0, int(0));
    int _S44 = int(high_0.x);

#line 1055
    int4 _S45 = int4(_S44, _S42, layer_0, int(0));
    int _S46 = int(high_0.y);

#line 1056
    int4 _S47 = int4(_S41, _S46, layer_0, int(0));
    int4 _S48 = int4(_S44, _S46, layer_0, int(0));
    float2 _S49 = float2(weight_0.x) ;

#line 1058
    return mix(mix(((kernelContext_10->probe_visibility_0).read(vec<uint,2>(((_S43)).xy), uint(((_S43)).z), uint(((_S43)).w))).xy, ((kernelContext_10->probe_visibility_0).read(vec<uint,2>(((_S45)).xy), uint(((_S45)).z), uint(((_S45)).w))).xy, _S49), mix(((kernelContext_10->probe_visibility_0).read(vec<uint,2>(((_S47)).xy), uint(((_S47)).z), uint(((_S47)).w))).xy, ((kernelContext_10->probe_visibility_0).read(vec<uint,2>(((_S48)).xy), uint(((_S48)).z), uint(((_S48)).w))).xy, _S49), float2(weight_0.y) );
}


#line 1073
float probe_chebyshev_0(uint index_1, float3 probe_position_0, float3 world_position_1, float3 normal_0, KernelContext_0 thread* kernelContext_11)
{
    float3 to_probe_0 = probe_position_0 - (world_position_1 + normal_0 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 1076
    float2 _S50 = probe_moments_0(index_1, - to_probe_0, kernelContext_11);

#line 1082
    float _S51 = _S50.x;

#line 1082
    float _S52 = max(_S50.y - _S51 * _S51, 0.0f);
    float behind_0 = to_surface_0 - _S51;
    float bound_0 = _S52 / (_S52 + behind_0 * behind_0);

#line 1084
    float _S53;
    if(to_surface_0 <= _S51)
    {

#line 1085
        _S53 = 1.0f;

#line 1085
    }
    else
    {

#line 1085
        _S53 = bound_0 * bound_0 * bound_0;

#line 1085
    }

#line 1085
    return _S53;
}


#line 1101
float probe_weight_0(uint index_2, float3 probe_position_1, float3 world_position_2, float3 normal_1, KernelContext_0 thread* kernelContext_12)
{

#line 1101
    float _S54 = probe_chebyshev_0(index_2, probe_position_1, world_position_2, normal_1, kernelContext_12);

    return max(_S54, 0.00009999999747379f);
}


#line 180
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 1141
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 1168
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_2, float3 origin_1, float3 spacing_0, float3 world_position_3, float3 normal_2, KernelContext_0 thread* kernelContext_13)
{

#line 1169
    uint _S55 = probe_row_0(level_2, cell_2, kernelContext_13);


    GpuProbe_natural_0 stored_0 = kernelContext_13->probes_0[_S55];

#line 1172
    float _S56 = probe_weight_0(_S55, origin_1 + float3(cell_2) * spacing_0, world_position_3, normal_2, kernelContext_13);



    thread WeightedProbe_0 corner_0;

#line 1176
    float4 _S57 = float4(_S56) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S57;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S57;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S57;
    (&corner_0)->weight_1 = _S56;
    return corner_0;
}


#line 1152
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S58 = float4(t_0) ;

#line 1155
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S58);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S58);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S58);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1266
float3 probe_level_environment_0(uint level_3, float3 world_position_4, float3 normal_3, float3 direction_2, KernelContext_0 thread* kernelContext_14)
{

#line 1266
    float3 _S59 = float3(1.0f) ;

    float3 _S60 = float3(0.0f, 0.0f, 0.0f);

#line 1268
    float3 last_1 = max(float3(kernelContext_14->camera_0->probe_counts_0.xyz) - _S59, _S60);



    float3 origin_2 = kernelContext_14->camera_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_14->camera_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_4 - origin_2) * inv_0, _S60, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S61 = uint3(base_0);
    uint3 _S62 = uint3(min(base_0 + _S59, last_1));

#line 1283
    float _S63 = inv_0.x;

#line 1283
    float _S64;

#line 1283
    if(_S63 != 0.0f)
    {

#line 1283
        _S64 = 1.0f / _S63;

#line 1283
    }
    else
    {

#line 1283
        _S64 = 0.0f;

#line 1283
    }
    float _S65 = inv_0.y;

#line 1284
    float _S66;

#line 1284
    if(_S65 != 0.0f)
    {

#line 1284
        _S66 = 1.0f / _S65;

#line 1284
    }
    else
    {

#line 1284
        _S66 = 0.0f;

#line 1284
    }
    float _S67 = inv_0.z;

#line 1285
    float _S68;

#line 1285
    if(_S67 != 0.0f)
    {

#line 1285
        _S68 = 1.0f / _S67;

#line 1285
    }
    else
    {

#line 1285
        _S68 = 0.0f;

#line 1285
    }

#line 1283
    float3 spacing_1 = float3(_S64, _S66, _S68);

#line 1292
    uint _S69 = _S61.x;

#line 1292
    uint _S70 = _S61.y;

#line 1292
    uint _S71 = _S61.z;

#line 1292
    WeightedProbe_0 _S72 = probe_corner_0(level_3, uint3(_S69, _S70, _S71), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);
    uint _S73 = _S62.x;

#line 1293
    WeightedProbe_0 _S74 = probe_corner_0(level_3, uint3(_S73, _S70, _S71), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1293
    float _S75 = f_0.x;

#line 1293
    thread WeightedProbe_0 _S76 = _S72;

#line 1293
    thread WeightedProbe_0 _S77 = _S74;

#line 1293
    WeightedProbe_0 _S78 = lerp_probe_0(&_S76, &_S77, _S75);
    uint _S79 = _S62.y;

#line 1294
    WeightedProbe_0 _S80 = probe_corner_0(level_3, uint3(_S69, _S79, _S71), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1294
    WeightedProbe_0 _S81 = probe_corner_0(level_3, uint3(_S73, _S79, _S71), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1294
    thread WeightedProbe_0 _S82 = _S80;

#line 1294
    thread WeightedProbe_0 _S83 = _S81;

#line 1294
    WeightedProbe_0 _S84 = lerp_probe_0(&_S82, &_S83, _S75);

    uint _S85 = _S62.z;

#line 1296
    WeightedProbe_0 _S86 = probe_corner_0(level_3, uint3(_S69, _S70, _S85), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1296
    WeightedProbe_0 _S87 = probe_corner_0(level_3, uint3(_S73, _S70, _S85), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1296
    thread WeightedProbe_0 _S88 = _S86;

#line 1296
    thread WeightedProbe_0 _S89 = _S87;

#line 1296
    WeightedProbe_0 _S90 = lerp_probe_0(&_S88, &_S89, _S75);

#line 1296
    WeightedProbe_0 _S91 = probe_corner_0(level_3, uint3(_S69, _S79, _S85), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1296
    WeightedProbe_0 _S92 = probe_corner_0(level_3, uint3(_S73, _S79, _S85), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1296
    thread WeightedProbe_0 _S93 = _S91;

#line 1296
    thread WeightedProbe_0 _S94 = _S92;

#line 1296
    WeightedProbe_0 _S95 = lerp_probe_0(&_S93, &_S94, _S75);



    float _S96 = f_0.y;

#line 1300
    thread WeightedProbe_0 _S97 = _S78;

#line 1300
    thread WeightedProbe_0 _S98 = _S84;

#line 1300
    WeightedProbe_0 _S99 = lerp_probe_0(&_S97, &_S98, _S96);

#line 1300
    thread WeightedProbe_0 _S100 = _S90;

#line 1300
    thread WeightedProbe_0 _S101 = _S95;

#line 1300
    WeightedProbe_0 _S102 = lerp_probe_0(&_S100, &_S101, _S96);

    float _S103 = f_0.z;

#line 1302
    thread WeightedProbe_0 _S104 = _S99;

#line 1302
    thread WeightedProbe_0 _S105 = _S102;

#line 1302
    WeightedProbe_0 _S106 = lerp_probe_0(&_S104, &_S105, _S103);

#line 1302
    float3 _S107 = float3(2.09439516067504883f) ;

#line 1308
    return max(float3(dot(_S106.sh_0.sh_r_0.xyz / _S107, direction_2) + _S106.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S106.sh_0.sh_g_0.xyz / _S107, direction_2) + _S106.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S106.sh_0.sh_b_0.xyz / _S107, direction_2) + _S106.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S106.weight_1) , _S60);
}


#line 1325
float3 probe_environment_0(float3 world_position_5, float3 normal_4, float3 direction_3, KernelContext_0 thread* kernelContext_15)
{
    if((kernelContext_15->camera_0->probe_counts_0.w) == 0U)
    {

#line 1328
        return float3(0.0f, 0.0f, 0.0f);
    }

#line 1336
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_5, kernelContext_15->camera_0->probe_level_origin_0[int(0)].xyz, kernelContext_15->camera_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_15->camera_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_15->camera_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1338
    float3 _S108 = probe_level_environment_0(level_4, world_position_5, normal_4, direction_3, kernelContext_15);


    if(share_0 >= 1.0f)
    {

#line 1342
        return _S108;
    }

#line 1342
    float3 _S109 = probe_level_environment_0(level_4 + 1U, world_position_5, normal_4, direction_3, kernelContext_15);

    return _S109 * float3((1.0f - share_0))  + _S108 * float3(share_0) ;
}


#line 846
float2 decode_fixed_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 858
float2 fixed_pair_at_0(texture2d<float, access::sample> table_0, float2 at_2)
{
    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (table_0).get_width(0)),(*((&height_1)) = (table_0).get_height(0));
    float2 extent_6 = float2(float(width_1), float(height_1));
    float2 scaled_1 = saturate(at_2) * extent_6 - float2(0.5f) ;

#line 864
    float2 _S110 = float2(1.0f) ;
    float2 _S111 = extent_6 - _S110;

#line 865
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S111);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S112 = int2(low_1);
    int2 _S113 = int2(min(low_1 + _S110, _S111));
    int _S114 = _S112.x;

#line 871
    int _S115 = _S112.y;

#line 871
    int3 _S116 = int3(_S114, _S115, int(0));
    int _S117 = _S113.x;

#line 872
    int3 _S118 = int3(_S117, _S115, int(0));
    float2 _S119 = float2(weight_2.x) ;
    int _S120 = _S113.y;

#line 874
    int3 _S121 = int3(_S114, _S120, int(0));
    int3 _S122 = int3(_S117, _S120, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S116)).xy), uint(((_S116)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S118)).xy), uint(((_S118)).z)))), _S119), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S121)).xy), uint(((_S121)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S122)).xy), uint(((_S122)).z)))), _S119), float2(weight_2.y) );
}


float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_16)
{
    return fixed_pair_at_0(kernelContext_16->sky_prefilter_0, float2(up_0, roughness_1));
}


#line 904
float3 sky_prefiltered_0(float3 direction_4, float roughness_2, KernelContext_0 thread* kernelContext_17)
{
    float up_1 = clamp(direction_4.y, -1.0f, 1.0f);

#line 906
    float2 _S123 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_17);

    bool _S124 = up_1 >= 0.0f;

#line 908
    float3 far_0;

#line 908
    if(_S124)
    {

#line 908
        far_0 = kernelContext_17->camera_0->sky_0[int(0)].xyz;

#line 908
    }
    else
    {

#line 908
        far_0 = kernelContext_17->camera_0->sky_0[int(2)].xyz;

#line 908
    }

#line 908
    float3 opposite_0;
    if(_S124)
    {

#line 909
        opposite_0 = kernelContext_17->camera_0->sky_0[int(2)].xyz;

#line 909
    }
    else
    {

#line 909
        opposite_0 = kernelContext_17->camera_0->sky_0[int(0)].xyz;

#line 909
    }
    float _S125 = _S123.x;

#line 910
    float _S126 = _S123.y;
    return kernelContext_17->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S125 - _S126))  + far_0 * float3(_S125)  + opposite_0 * float3(_S126) ;
}


#line 923
float3 sky_view_at_0(float up_2, float azimuth_cosine_0, KernelContext_0 thread* kernelContext_18)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_0 = clamp(up_2, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_0));

#line 927
    float _S127;
    if(clamped_0 >= 0.0f)
    {

#line 928
        _S127 = root_0;

#line 928
    }
    else
    {

#line 928
        _S127 = - root_0;

#line 928
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    float x0_0 = clamp(floor(across_0), 0.0f, 95.0f);

    float fx_0 = clamp(across_0 - x0_0, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S127, 0.0f, 1.0f) * 64.0f - 0.5f;
    float y0_0 = clamp(floor(down_0), 0.0f, 63.0f);

    float fy_0 = clamp(down_0 - y0_0, 0.0f, 1.0f);

    uint row0_0 = uint(y0_0) * 96U;
    uint row1_0 = uint(min(y0_0 + 1.0f, 63.0f)) * 96U;
    uint _S128 = uint(x0_0);

#line 942
    float3 _S129 = float3((1.0f - fx_0)) ;
    uint _S130 = uint(min(x0_0 + 1.0f, 95.0f));

#line 943
    float3 _S131 = float3(fx_0) ;


    return ((float4(*(kernelContext_18->sky_view_0+(row0_0 + _S128))) ).xyz * _S129 + (float4(*(kernelContext_18->sky_view_0+(row0_0 + _S130))) ).xyz * _S131) * float3((1.0f - fy_0))  + ((float4(*(kernelContext_18->sky_view_0+(row1_0 + _S128))) ).xyz * _S129 + (float4(*(kernelContext_18->sky_view_0+(row1_0 + _S130))) ).xyz * _S131) * float3(fy_0) ;
}


#line 956
float3 atmosphere_radiance_0(float3 direction_5, KernelContext_0 thread* kernelContext_19)
{
    float3 sun_0 = kernelContext_19->camera_0->atmosphere_0.xyz;
    float _S132 = direction_5.x;

#line 959
    float _S133 = direction_5.z;

#line 959
    float view_flat_0 = sqrt(_S132 * _S132 + _S133 * _S133);
    float _S134 = sun_0.x;

#line 960
    float _S135 = sun_0.z;

#line 960
    float sun_flat_0 = sqrt(_S134 * _S134 + _S135 * _S135);

#line 960
    bool _S136;

    if(view_flat_0 > 0.0f)
    {

#line 962
        _S136 = sun_flat_0 > 0.0f;

#line 962
    }
    else
    {

#line 962
        _S136 = false;

#line 962
    }

#line 962
    float cosine_0;

#line 962
    if(_S136)
    {

#line 962
        cosine_0 = (_S132 * _S134 + _S133 * _S135) / (view_flat_0 * sun_flat_0);

#line 962
    }
    else
    {

#line 962
        cosine_0 = 1.0f;

#line 962
    }

#line 962
    float3 _S137 = sky_view_at_0(direction_5.y, cosine_0, kernelContext_19);



    return _S137;
}


#line 996
float3 sky_environment_0(float3 direction_6, float roughness_3, float share_1, KernelContext_0 thread* kernelContext_20)
{

#line 996
    float3 _S138 = sky_prefiltered_0(direction_6, roughness_3, kernelContext_20);


    if((kernelContext_20->camera_0->atmosphere_0.w) <= 0.0f)
    {
        return _S138;
    }



    float3 _S139 = _S138 * float3((1.0f - share_1)) ;

#line 1006
    float3 _S140 = atmosphere_radiance_0(direction_6, kernelContext_20);

#line 1006
    return _S139 + _S140 * float3(share_1) ;
}


#line 887
float2 dfg_at_0(float n_dot_v_0, float roughness_4, KernelContext_0 thread* kernelContext_21)
{
    return fixed_pair_at_0(kernelContext_21->dfg_0, float2(n_dot_v_0, roughness_4));
}


#line 709
float2 pixel_of_0(float2 ndc_1, float2 size_1)
{
    return float2((ndc_1.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_1.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_3, float2 size_2)
{
    return float2(at_3.x / size_2.x * 2.0f - 1.0f, 1.0f - at_3.y / size_2.y * 2.0f);
}


#line 786
float cell_exit_0(float2 at_4, float2 forward_0, float size_3, float reach_2)
{

    float _S141 = forward_0.x;

#line 789
    bool _S142 = _S141 > 0.0f;

#line 789
    float along_x_0;

#line 789
    if(_S142)
    {

#line 789
        along_x_0 = (floor(at_4.x / size_3) + 1.0f) * size_3;

#line 789
    }
    else
    {

#line 789
        along_x_0 = floor(at_4.x / size_3) * size_3;

#line 789
    }
    float _S143 = forward_0.y;

#line 790
    bool _S144 = _S143 > 0.0f;

#line 790
    float along_y_0;

#line 790
    if(_S144)
    {

#line 790
        along_y_0 = (floor(at_4.y / size_3) + 1.0f) * size_3;

#line 790
    }
    else
    {

#line 790
        along_y_0 = floor(at_4.y / size_3) * size_3;

#line 790
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 791
    float _S145;

    if((abs(_S141)) < 9.99999997475242708e-07f)
    {

#line 793
        along_x_0 = reach_2;

#line 793
    }
    else
    {

#line 794
        if(_S142)
        {

#line 794
            _S145 = nudge_0;

#line 794
        }
        else
        {

#line 794
            _S145 = - nudge_0;

#line 794
        }

#line 794
        along_x_0 = (along_x_0 + _S145 - at_4.x) / _S141;

#line 793
    }


    if((abs(_S143)) < 9.99999997475242708e-07f)
    {

#line 796
        along_y_0 = reach_2;

#line 796
    }
    else
    {

#line 797
        if(_S144)
        {

#line 797
            _S145 = nudge_0;

#line 797
        }
        else
        {

#line 797
            _S145 = - nudge_0;

#line 797
        }

#line 797
        along_y_0 = (along_y_0 + _S145 - at_4.y) / _S143;

#line 796
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 745
float hiz_at_0(uint level_5, int2 texel_1, int2 extent_7, KernelContext_0 thread* kernelContext_22)
{
    int2 _S146 = int2(int(0), int(0));
    int3 at_5 = int3(clamp(texel_1, _S146, max(extent_7 - int2(int(1), int(1)), _S146)), int(0));
    switch(level_5)
    {
    case 0U:
        {

#line 752
            return ((kernelContext_22->scene_depth_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    case 1U:
        {

#line 754
            return ((kernelContext_22->hiz_1_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    case 2U:
        {

#line 756
            return ((kernelContext_22->hiz_2_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    case 3U:
        {

#line 758
            return ((kernelContext_22->hiz_3_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    case 4U:
        {

#line 760
            return ((kernelContext_22->hiz_4_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    default:
        {

#line 762
            return ((kernelContext_22->hiz_5_0).read(vec<uint,2>(((at_5)).xy), uint(((at_5)).z)));
        }
    }

#line 762
}


#line 773
float view_z_of_0(float depth_5, KernelContext_0 thread* kernelContext_23)
{

#line 773
    float2 _S147 = unproject_z_1(depth_5, kernelContext_23);


    return _S147.x / _S147.y;
}


#line 728
float thickness_at_0(float advance_0, float depth_6)
{
    return max(advance_0, abs(depth_6) * 0.01999999955296516f);
}


#line 730
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 730
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 1359
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S148 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], packed_float4 device* sky_view_1 [[buffer(2)]], texture2d<float, access::sample> dfg_1 [[texture(9)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 1359
    float3 reflection_0;

#line 1359
    thread KernelContext_0 kernelContext_24;

#line 1359
    (&kernelContext_24)->scene_depth_0 = scene_depth_1;

#line 1359
    (&kernelContext_24)->reflectivity_0 = reflectivity_1;

#line 1359
    (&kernelContext_24)->camera_0 = camera_1;

#line 1359
    (&kernelContext_24)->probes_0 = probes_1;

#line 1359
    (&kernelContext_24)->probe_visibility_0 = probe_visibility_1;

#line 1359
    (&kernelContext_24)->sky_prefilter_0 = sky_prefilter_1;

#line 1359
    (&kernelContext_24)->sky_view_0 = sky_view_1;

#line 1359
    (&kernelContext_24)->dfg_0 = dfg_1;

#line 1359
    (&kernelContext_24)->hiz_1_0 = hiz_1_1;

#line 1359
    (&kernelContext_24)->hiz_2_0 = hiz_2_1;

#line 1359
    (&kernelContext_24)->hiz_3_0 = hiz_3_1;

#line 1359
    (&kernelContext_24)->hiz_4_0 = hiz_4_1;

#line 1359
    (&kernelContext_24)->hiz_5_0 = hiz_5_1;

#line 1359
    (&kernelContext_24)->scene_color_0 = scene_color_1;

    thread uint width_2;
    thread uint height_2;



    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    int _S149 = int(width_2);

#line 1367
    int _S150 = int(height_2);

#line 1367
    int2 extent_8 = int2(_S149, _S150);
    float _S151 = float(width_2);

#line 1368
    float _S152 = float(height_2);

#line 1368
    float2 size_4 = float2(_S151, _S152);

#line 1368
    int2 _S153 = representative_pixel_0(int2(position_0.xy), extent_8, &kernelContext_24);

#line 1376
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S154 = int3(_S153, int(0));

#line 1378
    float4 surface_0 = (((&kernelContext_24)->reflectivity_0).read(vec<uint,2>(((_S154)).xy), uint(((_S154)).z)));
    float _S155 = surface_0.w;

#line 1379
    float sharpness_0 = sharpness_of_0(_S155);

#line 1379
    float _S156 = depth_at_0(_S153, extent_8, &kernelContext_24);


    if(_S156 <= 0.0f)
    {

#line 1382
        pixelOutput_0 _S157 = { NOTHING_0 };

        return _S157;
    }

#line 1384
    float3 _S158 = view_position_0(_S153, _S156, size_4, &kernelContext_24);

#line 1384
    float3 _S159 = normal_at_0(_S153, _S158, extent_8, size_4, &kernelContext_24);

#line 1390
    float3 towards_0 = normalize(_S158);
    float3 ray_0 = reflect(towards_0, _S159);


    float4 _S160 = float4(ray_0, 0.0f);

#line 1394
    float3 reflection_direction_0 = normalize((((_S160) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 1394
    float3 _S161 = probe_environment_0((((float4(_S158, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, normalize((((float4(_S159, 0.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz), reflection_direction_0, &kernelContext_24);

#line 1394
    float3 _S162 = sky_environment_0(reflection_direction_0, _S155, sharpness_0, &kernelContext_24);

#line 1417
    float3 environment_0 = _S161 + _S162;

#line 1425
    float3 _S163 = - towards_0;
    float3 f0_0 = surface_0.xyz;

#line 1426
    float2 _S164 = dfg_at_0(saturate(dot(_S159, _S163)), _S155, &kernelContext_24);

    float3 env_brdf_0 = f0_0 * float3(_S164.x)  + float3(_S164.y) ;

#line 1433
    if(sharpness_0 <= 0.0f)
    {

#line 1433
        pixelOutput_0 _S165 = { float4(environment_0 * env_brdf_0, 0.0f) };

        return _S165;
    }


    float _S166 = saturate((1.0f - dot(ray_0, _S163)) / 0.05000000074505806f);


    float _S167 = _S158.z;

#line 1442
    float3 start_0 = _S158 + _S159 * float3((abs(_S167) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S160) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S168 = clip_start_0.w;

#line 1447
    if(_S168 <= 0.0f)
    {

#line 1447
        pixelOutput_0 _S169 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S169;
    }
    float2 _S170 = clip_start_0.xy;

#line 1451
    float2 _S171 = float2(_S168) ;

#line 1451
    float2 at_start_0 = pixel_of_0(_S170 / _S171, size_4);

#line 1457
    float2 _S172 = clip_ray_0.xy;

#line 1457
    float _S173 = clip_ray_0.w;

#line 1457
    float2 _S174 = float2(_S173) ;

#line 1457
    float2 ndc_rate_0 = (_S172 * _S171 - _S170 * _S174) / float2((_S168 * _S168)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S151, - ndc_rate_0.y * 0.5f * _S152);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 1460
        pixelOutput_0 _S175 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S175;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 1471
    float reach_3 = 0.75f * min(_S151, _S152);

    float _S176 = forward_1.x;

#line 1473
    float travel_0;

#line 1473
    if(_S176 > 0.0f)
    {

#line 1473
        travel_0 = min(reach_3, (_S151 - 1.0f - at_start_0.x) / _S176);

#line 1473
    }
    else
    {

        if(_S176 < 0.0f)
        {

#line 1477
            travel_0 = min(reach_3, - at_start_0.x / _S176);

#line 1477
        }
        else
        {

#line 1477
            travel_0 = reach_3;

#line 1477
        }

#line 1473
    }

#line 1481
    float _S177 = forward_1.y;

#line 1481
    if(_S177 > 0.0f)
    {

#line 1481
        travel_0 = min(travel_0, (_S152 - 1.0f - at_start_0.y) / _S177);

#line 1481
    }
    else
    {

        if(_S177 < 0.0f)
        {

#line 1485
            travel_0 = min(travel_0, - at_start_0.y / _S177);

#line 1485
        }

#line 1481
    }

#line 1493
    if(_S173 > 0.0f)
    {

#line 1493
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S172 / _S174, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 1493
    }
    else
    {

#line 1508
        if(_S173 < 0.0f)
        {

#line 1515
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 1520
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S168) / _S173)) ;

#line 1520
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 1508
        }

#line 1493
    }

#line 1527
    float _S178 = max(travel_0, 0.0f);
    if(_S178 <= 0.00390625f)
    {

#line 1528
        pixelOutput_0 _S179 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S179;
    }

#line 1537
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S178) , size_4);

#line 1537
    float when_end_0;

    if((abs(_S176)) >= (abs(_S177)))
    {

#line 1539
        float _S180 = ndc_end_0.x;

#line 1539
        when_end_0 = (_S180 * _S168 - clip_start_0.x) / (clip_ray_0.x - _S180 * _S173);

#line 1539
    }
    else
    {

#line 1540
        float _S181 = ndc_end_0.y;

#line 1540
        when_end_0 = (_S181 * _S168 - clip_start_0.y) / (clip_ray_0.y - _S181 * _S173);

#line 1539
    }

#line 1539
    bool _S182;

#line 1547
    if(!(when_end_0 > 0.0f))
    {

#line 1547
        _S182 = true;

#line 1547
    }
    else
    {

#line 1547
        _S182 = !isfinite(when_end_0);

#line 1547
    }

#line 1547
    if(_S182)
    {

#line 1547
        pixelOutput_0 _S183 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S183;
    }

#line 1555
    float inverse_w_start_0 = 1.0f / _S168;

    float inverse_w_end_0 = 1.0f / (_S168 + when_end_0 * _S173);
    float _S184 = start_0.z;

#line 1558
    float _S185 = _S184 * inverse_w_start_0;
    float _S186 = (_S184 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 1564
    float3 _S187 = environment_0 * env_brdf_0;
    uint _S188 = min((&kernelContext_24)->camera_0->hiz_0.x, 5U);

#line 1595
    float _S189 = _S184 - _S167;

#line 1595
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S178), _S178);

#line 1595
    float previous_gap_0 = _S189;

#line 1595
    float entry_z_0 = _S184;

#line 1595
    uint step_0 = 0U;

#line 1595
    uint level_6 = 0U;

    for(;;)
    {

#line 1597
        if(step_0 < 96U)
        {
        }
        else
        {

#line 1597
            reflection_0 = _S187;

#line 1597
            break;
        }
        float cell_3 = float(1U << level_6);
        float2 at_6 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S190 = min(at_travel_0 + cell_exit_0(at_6, forward_1, cell_3, _S178), _S178);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S190) ;
        float along_0 = _S190 / _S178;

        float exit_z_0 = mix(_S185, _S186, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 1605
        float _S191 = hiz_at_0(level_6, int2(floor(at_6 / float2(cell_3) )), int2(_S149 >> level_6, _S150 >> level_6), &kernelContext_24);

#line 1605
        float gap_0;

#line 1614
        if(_S191 <= 0.0f)
        {

#line 1614
            gap_0 = 1.0f;

#line 1614
        }
        else
        {

#line 1614
            float _S192 = view_z_of_0(_S191, &kernelContext_24);

#line 1614
            gap_0 = exit_z_0 - _S192;

#line 1614
        }

#line 1623
        bool _S193 = !(gap_0 > 0.0f);

#line 1623
        if(_S193)
        {

#line 1623
            _S182 = level_6 > 0U;

#line 1623
        }
        else
        {

#line 1623
            _S182 = false;

#line 1623
        }

#line 1623
        if(_S182)
        {

#line 1623
            level_6 = level_6 - 1U;

#line 1629
            step_0 = step_0 + 1U;

#line 1597
            continue;
        }

#line 1597
        bool _S194;

#line 1632
        if(_S193)
        {

#line 1632
            _S194 = previous_gap_0 > 0.0f;

#line 1632
        }
        else
        {

#line 1632
            _S194 = false;

#line 1632
        }

#line 1632
        if(_S194)
        {



            float behind_1 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_1 <= thickness_0)
            {

#line 1645
                float2 hit_at_0 = mix(at_6, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1660
                float confidence_0 = sharpness_0 * _S166 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S190 / reach_3) / 0.25f) * saturate(1.0f - behind_1 / thickness_0);
                int3 _S195 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_8 - int2(int(1), int(1))), int(0));

#line 1661
                reflection_0 = (((&kernelContext_24)->scene_color_0).read(vec<uint,2>(((_S195)).xy), uint(((_S195)).z))).xyz * env_brdf_0 * float3(confidence_0)  + _S187 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 1632
        }

#line 1673
        if(_S190 >= _S178)
        {

#line 1673
            reflection_0 = _S187;

            break;
        }



        uint _S196 = min(level_6 + 1U, _S188);

#line 1680
        at_travel_0 = _S190;

#line 1680
        previous_gap_0 = gap_0;

#line 1680
        entry_z_0 = exit_z_0;

#line 1680
        level_6 = _S196;

#line 1597
        step_0 = step_0 + 1U;

#line 1597
    }

#line 1597
    pixelOutput_0 _S197 = { float4(reflection_0, sharpness_0) };

#line 1688
    return _S197;
}


#line 1688
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 540
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 540
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(8)]], packed_float4 device* sky_view_2 [[buffer(2)]], texture2d<float, access::sample> dfg_2 [[texture(9)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 540
    thread KernelContext_0 kernelContext_25;

#line 540
    (&kernelContext_25)->scene_depth_0 = scene_depth_2;

#line 540
    (&kernelContext_25)->reflectivity_0 = reflectivity_2;

#line 540
    (&kernelContext_25)->camera_0 = camera_2;

#line 540
    (&kernelContext_25)->probes_0 = probes_2;

#line 540
    (&kernelContext_25)->probe_visibility_0 = probe_visibility_2;

#line 540
    (&kernelContext_25)->sky_prefilter_0 = sky_prefilter_2;

#line 540
    (&kernelContext_25)->sky_view_0 = sky_view_2;

#line 540
    (&kernelContext_25)->dfg_0 = dfg_2;

#line 540
    (&kernelContext_25)->hiz_1_0 = hiz_1_2;

#line 540
    (&kernelContext_25)->hiz_2_0 = hiz_2_2;

#line 540
    (&kernelContext_25)->hiz_3_0 = hiz_3_2;

#line 540
    (&kernelContext_25)->hiz_4_0 = hiz_4_2;

#line 540
    (&kernelContext_25)->hiz_5_0 = hiz_5_2;

#line 540
    (&kernelContext_25)->scene_color_0 = scene_color_2;

#line 1350
    thread FullscreenOutput_0 output_1;


    float2 _S198 = float2(float((index_3 << 1U) & 2U), float(index_3 & 2U));

#line 1353
    (&output_1)->uv_2 = _S198;
    (&output_1)->position_2 = float4(_S198 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 1354
    thread vertexMain_Result_0 _S199;

#line 1354
    (&_S199)->position_1 = output_1.position_2;

#line 1354
    (&_S199)->uv_1 = output_1.uv_2;

#line 1354
    return _S199;
}

