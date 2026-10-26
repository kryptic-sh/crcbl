#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 189 "shaders/ssr.slang"
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


#line 232
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 215
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    SsrParams_natural_0 constant* camera_0;
    texture2d<float, access::sample> reflectivity_0;
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


#line 556
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 559
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 556
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 559
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 574
int2 representative_pixel_0(int2 block_0, int2 extent_2, int2 working_0, KernelContext_0 thread* kernelContext_2)
{
    int2 _S3 = block_0 * extent_2 / working_0;

#line 576
    int2 _S4 = int2(int(1), int(1));

#line 576
    int2 first_0 = clamp(_S3, int2(int(0), int(0)), extent_2 - _S4);
    int2 last_0 = (block_0 + _S4) * extent_2 / working_0;
    int2 _S5 = clamp(last_0, first_0 + _S4, min(first_0 + int2(int(4), int(4)), extent_2));

#line 578
    float _S6 = depth_at_1(first_0, extent_2, kernelContext_2);

#line 578
    float best_depth_0 = _S6;

#line 578
    int2 best_0 = first_0;

#line 578
    int y_0 = int(0);


    for(;;)
    {

#line 581
        if(y_0 < int(4))
        {
        }
        else
        {

#line 581
            break;
        }

#line 581
        float best_depth_1 = best_depth_0;

#line 581
        int2 best_1 = best_0;

#line 581
        int x_0 = int(0);

        for(;;)
        {

#line 583
            if(x_0 < int(4))
            {
            }
            else
            {

#line 583
                break;
            }
            int2 candidate_0 = first_0 + int2(x_0, y_0);

#line 585
            bool _S7;
            if((candidate_0.x) < (_S5.x))
            {

#line 586
                _S7 = (candidate_0.y) < (_S5.y);

#line 586
            }
            else
            {

#line 586
                _S7 = false;

#line 586
            }

#line 586
            if(_S7)
            {

#line 586
                float _S8 = depth_at_1(candidate_0, extent_2, kernelContext_2);

#line 586
                float best_depth_2;

#line 586
                int2 best_2;


                if(_S8 > best_depth_1)
                {

#line 589
                    best_depth_2 = _S8;

#line 589
                    best_2 = candidate_0;

#line 589
                }
                else
                {

#line 589
                    best_depth_2 = best_depth_1;

#line 589
                    best_2 = best_1;

#line 589
                }

#line 589
                best_depth_1 = best_depth_2;

#line 589
                best_1 = best_2;

#line 586
            }

#line 583
            x_0 = x_0 + int(1);

#line 583
        }

#line 581
        int y_1 = y_0 + int(1);

#line 581
        best_depth_0 = best_depth_1;

#line 581
        best_0 = best_1;

#line 581
        y_0 = y_1;

#line 581
    }

#line 597
    return best_0;
}


#line 410
float sharpness_of_0(float roughness_0)
{
    return saturate(1.0f - roughness_0 / 0.5f);
}


#line 615
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_3)
{
    return float2((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 615
float2 unproject_z_1(float depth_1, KernelContext_0 thread* kernelContext_4)
{
    return float2((&kernelContext_4->camera_0->inv_proj_0)->data_0[int(2)].z * depth_1 + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(2)].w * depth_1 + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 646
float4 unproject_0(float2 ndc_0, float depth_2, KernelContext_0 thread* kernelContext_5)
{

#line 646
    float2 _S9 = unproject_z_0(depth_2, kernelContext_5);


    return float4((&kernelContext_5->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_5->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_5->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_5->camera_0->inv_proj_0)->data_0[int(3)].y, _S9.x, _S9.y);
}


#line 662
float3 view_position_0(int2 pixel_2, float depth_3, float2 extent_3, KernelContext_0 thread* kernelContext_6)
{

#line 662
    float4 _S10 = unproject_0(float2((float(pixel_2.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_3.y * 2.0f), depth_3, kernelContext_6);

#line 673
    return _S10.xyz / float3(_S10.w) ;
}


#line 662
float3 view_position_1(int2 pixel_3, float depth_4, float2 extent_4, KernelContext_0 thread* kernelContext_7)
{

#line 662
    float4 _S11 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_4.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_4.y * 2.0f), depth_4, kernelContext_7);

#line 673
    return _S11.xyz / float3(_S11.w) ;
}


#line 688
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_5, float2 size_0, KernelContext_0 thread* kernelContext_8)
{
    int2 _S12 = pixel_4 + int2(int(-1), int(0));

#line 690
    float _S13 = depth_at_1(_S12, extent_5, kernelContext_8);

#line 690
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_8);
    int2 _S15 = pixel_4 + int2(int(1), int(0));

#line 691
    float _S16 = depth_at_1(_S15, extent_5, kernelContext_8);

#line 691
    float3 _S17 = view_position_1(_S15, _S16, size_0, kernelContext_8);
    int2 _S18 = pixel_4 + int2(int(0), int(-1));

#line 692
    float _S19 = depth_at_1(_S18, extent_5, kernelContext_8);

#line 692
    float3 _S20 = view_position_1(_S18, _S19, size_0, kernelContext_8);
    int2 _S21 = pixel_4 + int2(int(0), int(1));

#line 693
    float _S22 = depth_at_1(_S21, extent_5, kernelContext_8);

#line 693
    float3 _S23 = view_position_1(_S21, _S22, size_0, kernelContext_8);

    float _S24 = centre_0.z;

#line 695
    float3 horizontal_0;
    if((abs(_S17.z - _S24)) < (abs(_S24 - _S14.z)))
    {

#line 696
        horizontal_0 = _S17 - centre_0;

#line 696
    }
    else
    {

#line 696
        horizontal_0 = centre_0 - _S14;

#line 696
    }

#line 696
    float3 vertical_0;


    if((abs(_S23.z - _S24)) < (abs(_S24 - _S20.z)))
    {

#line 699
        vertical_0 = _S23 - centre_0;

#line 699
    }
    else
    {

#line 699
        vertical_0 = centre_0 - _S20;

#line 699
    }

#line 709
    return normalize(cross(vertical_0, horizontal_0));
}


#line 1229
float probe_level_reach_0(float3 world_position_0, float3 origin_0, float3 inv_spacing_0, float3 last_1)
{

#line 1229
    float reach_0 = 0.0f;

#line 1229
    uint axis_0 = 0U;


    for(;;)
    {

#line 1232
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 1232
            break;
        }

#line 1232
        uint _S25 = axis_0;

#line 1232
        bool _S26;

        if((last_1[axis_0]) == 0.0f)
        {

#line 1234
            _S26 = true;

#line 1234
        }
        else
        {

#line 1234
            _S26 = (inv_spacing_0[axis_0]) == 0.0f;

#line 1234
        }

#line 1234
        if(_S26)
        {

#line 1235
            axis_0 = axis_0 + 1U;

#line 1232
            continue;
        }

#line 1232
        reach_0 = max(reach_0, abs(2.0f * ((world_position_0[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_1[_S25] - 1.0f));

#line 1232
        axis_0 = axis_0 + 1U;

#line 1232
    }

#line 1239
    return reach_0;
}


#line 1249
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 1249
    uint level_0 = 0U;

    for(;;)
    {

#line 1251
        uint _S27 = level_0 + 1U;

#line 1251
        if(_S27 < levels_0)
        {
        }
        else
        {

#line 1251
            break;
        }
        float _S28 = float(level_0);

#line 1253
        float at_0 = reach_1 * exp2(- _S28);
        if(at_0 < 1.0f)
        {

#line 1255
            return float2(_S28, saturate((1.0f - at_0) / 0.25f));
        }

#line 1251
        level_0 = _S27;

#line 1251
    }

#line 1257
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 1138
uint probe_wrap_0(uint cell_0, uint offset_0, uint count_0)
{
    uint at_1 = cell_0 + offset_0;

#line 1140
    uint _S29;
    if(at_1 >= count_0)
    {

#line 1141
        _S29 = at_1 - count_0;

#line 1141
    }
    else
    {

#line 1141
        _S29 = at_1;

#line 1141
    }

#line 1141
    return _S29;
}


#line 1154
uint probe_row_0(uint level_1, uint3 cell_1, KernelContext_0 thread* kernelContext_9)
{
    uint3 counts_0 = kernelContext_9->camera_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_9->camera_0->probe_level_offset_0[level_1].xyz;
    uint _S30 = counts_0.x;
    uint _S31 = counts_0.y;



    return min(kernelContext_9->camera_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_1.z, offset_1.z, counts_0.z) * _S31 + probe_wrap_0(cell_1.y, offset_1.y, _S31)) * _S30 + probe_wrap_0(cell_1.x, offset_1.x, _S30), max(kernelContext_9->camera_0->probe_counts_0.w, 1U) - 1U);
}


#line 1040
float sign_not_zero_0(float value_0)
{

#line 1040
    float _S32;

    if(value_0 >= 0.0f)
    {

#line 1042
        _S32 = 1.0f;

#line 1042
    }
    else
    {

#line 1042
        _S32 = -1.0f;

#line 1042
    }

#line 1042
    return _S32;
}


#line 1050
float2 oct_encode_0(float3 direction_0)
{
    float _S33 = direction_0.y;
    float2 p_0 = direction_0.xz / float2(max(abs(direction_0.x) + abs(_S33) + abs(direction_0.z), 9.99999968265522539e-21f)) ;

#line 1053
    float2 p_1;
    if(_S33 < 0.0f)
    {
        float _S34 = p_0.y;

#line 1056
        float _S35 = p_0.x;

#line 1056
        p_1 = float2((1.0f - abs(_S34)) * sign_not_zero_0(_S35), (1.0f - abs(_S35)) * sign_not_zero_0(_S34));

#line 1054
    }
    else
    {

#line 1054
        p_1 = p_0;

#line 1054
    }

#line 1059
    return p_1;
}


#line 1068
float2 probe_moments_0(uint index_0, float3 direction_1, KernelContext_0 thread* kernelContext_10)
{

#line 1068
    texture2d_array<float, access::sample> _S36 = kernelContext_10->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S36).get_width(0)),(*((&height_0)) = (_S36).get_height(0)),(*((&layers_0)) = (_S36).get_array_size());

#line 1073
    float2 _S37 = float2(0.5f) ;

#line 1073
    float2 _S38 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_1) * _S37 + _S37) * float2(16.0f)  + _S38 - _S37;
    float2 _S39 = float2(float(width_0), float(height_0)) - _S38;

#line 1077
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S39);
    float2 high_0 = min(low_0 + _S38, _S39);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_0, max(layers_0, 1U) - 1U));

    int _S40 = int(low_0.x);

#line 1082
    int _S41 = int(low_0.y);

#line 1082
    int4 _S42 = int4(_S40, _S41, layer_0, int(0));
    int _S43 = int(high_0.x);

#line 1083
    int4 _S44 = int4(_S43, _S41, layer_0, int(0));
    int _S45 = int(high_0.y);

#line 1084
    int4 _S46 = int4(_S40, _S45, layer_0, int(0));
    int4 _S47 = int4(_S43, _S45, layer_0, int(0));
    float2 _S48 = float2(weight_0.x) ;

#line 1086
    return mix(mix(((kernelContext_10->probe_visibility_0).read(vec<uint,2>(((_S42)).xy), uint(((_S42)).z), uint(((_S42)).w))).xy, ((kernelContext_10->probe_visibility_0).read(vec<uint,2>(((_S44)).xy), uint(((_S44)).z), uint(((_S44)).w))).xy, _S48), mix(((kernelContext_10->probe_visibility_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z), uint(((_S46)).w))).xy, ((kernelContext_10->probe_visibility_0).read(vec<uint,2>(((_S47)).xy), uint(((_S47)).z), uint(((_S47)).w))).xy, _S48), float2(weight_0.y) );
}


#line 1101
float probe_chebyshev_0(uint index_1, float3 probe_position_0, float3 world_position_1, float3 normal_0, KernelContext_0 thread* kernelContext_11)
{
    float3 to_probe_0 = probe_position_0 - (world_position_1 + normal_0 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 1104
    float2 _S49 = probe_moments_0(index_1, - to_probe_0, kernelContext_11);

#line 1110
    float _S50 = _S49.x;

#line 1110
    float _S51 = max(_S49.y - _S50 * _S50, 0.0f);
    float behind_0 = to_surface_0 - _S50;
    float bound_0 = _S51 / (_S51 + behind_0 * behind_0);

#line 1112
    float _S52;
    if(to_surface_0 <= _S50)
    {

#line 1113
        _S52 = 1.0f;

#line 1113
    }
    else
    {

#line 1113
        _S52 = bound_0 * bound_0 * bound_0;

#line 1113
    }

#line 1113
    return _S52;
}


#line 1129
float probe_weight_0(uint index_2, float3 probe_position_1, float3 world_position_2, float3 normal_1, KernelContext_0 thread* kernelContext_12)
{

#line 1129
    float _S53 = probe_chebyshev_0(index_2, probe_position_1, world_position_2, normal_1, kernelContext_12);

    return max(_S53, 0.00009999999747379f);
}


#line 181
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 1169
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 1196
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_2, float3 origin_1, float3 spacing_0, float3 world_position_3, float3 normal_2, KernelContext_0 thread* kernelContext_13)
{

#line 1197
    uint _S54 = probe_row_0(level_2, cell_2, kernelContext_13);


    GpuProbe_natural_0 stored_0 = kernelContext_13->probes_0[_S54];

#line 1200
    float _S55 = probe_weight_0(_S54, origin_1 + float3(cell_2) * spacing_0, world_position_3, normal_2, kernelContext_13);



    thread WeightedProbe_0 corner_0;

#line 1204
    float4 _S56 = float4(_S55) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S56;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S56;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S56;
    (&corner_0)->weight_1 = _S55;
    return corner_0;
}


#line 1180
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S57 = float4(t_0) ;

#line 1183
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S57);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S57);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S57);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1294
float3 probe_level_environment_0(uint level_3, float3 world_position_4, float3 normal_3, float3 direction_2, KernelContext_0 thread* kernelContext_14)
{

#line 1294
    float3 _S58 = float3(1.0f) ;

    float3 _S59 = float3(0.0f, 0.0f, 0.0f);

#line 1296
    float3 last_2 = max(float3(kernelContext_14->camera_0->probe_counts_0.xyz) - _S58, _S59);



    float3 origin_2 = kernelContext_14->camera_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_14->camera_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_4 - origin_2) * inv_0, _S59, last_2);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S60 = uint3(base_0);
    uint3 _S61 = uint3(min(base_0 + _S58, last_2));

#line 1311
    float _S62 = inv_0.x;

#line 1311
    float _S63;

#line 1311
    if(_S62 != 0.0f)
    {

#line 1311
        _S63 = 1.0f / _S62;

#line 1311
    }
    else
    {

#line 1311
        _S63 = 0.0f;

#line 1311
    }
    float _S64 = inv_0.y;

#line 1312
    float _S65;

#line 1312
    if(_S64 != 0.0f)
    {

#line 1312
        _S65 = 1.0f / _S64;

#line 1312
    }
    else
    {

#line 1312
        _S65 = 0.0f;

#line 1312
    }
    float _S66 = inv_0.z;

#line 1313
    float _S67;

#line 1313
    if(_S66 != 0.0f)
    {

#line 1313
        _S67 = 1.0f / _S66;

#line 1313
    }
    else
    {

#line 1313
        _S67 = 0.0f;

#line 1313
    }

#line 1311
    float3 spacing_1 = float3(_S63, _S65, _S67);

#line 1320
    uint _S68 = _S60.x;

#line 1320
    uint _S69 = _S60.y;

#line 1320
    uint _S70 = _S60.z;

#line 1320
    WeightedProbe_0 _S71 = probe_corner_0(level_3, uint3(_S68, _S69, _S70), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);
    uint _S72 = _S61.x;

#line 1321
    WeightedProbe_0 _S73 = probe_corner_0(level_3, uint3(_S72, _S69, _S70), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1321
    float _S74 = f_0.x;

#line 1321
    thread WeightedProbe_0 _S75 = _S71;

#line 1321
    thread WeightedProbe_0 _S76 = _S73;

#line 1321
    WeightedProbe_0 _S77 = lerp_probe_0(&_S75, &_S76, _S74);
    uint _S78 = _S61.y;

#line 1322
    WeightedProbe_0 _S79 = probe_corner_0(level_3, uint3(_S68, _S78, _S70), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1322
    WeightedProbe_0 _S80 = probe_corner_0(level_3, uint3(_S72, _S78, _S70), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1322
    thread WeightedProbe_0 _S81 = _S79;

#line 1322
    thread WeightedProbe_0 _S82 = _S80;

#line 1322
    WeightedProbe_0 _S83 = lerp_probe_0(&_S81, &_S82, _S74);

    uint _S84 = _S61.z;

#line 1324
    WeightedProbe_0 _S85 = probe_corner_0(level_3, uint3(_S68, _S69, _S84), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1324
    WeightedProbe_0 _S86 = probe_corner_0(level_3, uint3(_S72, _S69, _S84), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1324
    thread WeightedProbe_0 _S87 = _S85;

#line 1324
    thread WeightedProbe_0 _S88 = _S86;

#line 1324
    WeightedProbe_0 _S89 = lerp_probe_0(&_S87, &_S88, _S74);

#line 1324
    WeightedProbe_0 _S90 = probe_corner_0(level_3, uint3(_S68, _S78, _S84), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1324
    WeightedProbe_0 _S91 = probe_corner_0(level_3, uint3(_S72, _S78, _S84), origin_2, spacing_1, world_position_4, normal_3, kernelContext_14);

#line 1324
    thread WeightedProbe_0 _S92 = _S90;

#line 1324
    thread WeightedProbe_0 _S93 = _S91;

#line 1324
    WeightedProbe_0 _S94 = lerp_probe_0(&_S92, &_S93, _S74);



    float _S95 = f_0.y;

#line 1328
    thread WeightedProbe_0 _S96 = _S77;

#line 1328
    thread WeightedProbe_0 _S97 = _S83;

#line 1328
    WeightedProbe_0 _S98 = lerp_probe_0(&_S96, &_S97, _S95);

#line 1328
    thread WeightedProbe_0 _S99 = _S89;

#line 1328
    thread WeightedProbe_0 _S100 = _S94;

#line 1328
    WeightedProbe_0 _S101 = lerp_probe_0(&_S99, &_S100, _S95);

    float _S102 = f_0.z;

#line 1330
    thread WeightedProbe_0 _S103 = _S98;

#line 1330
    thread WeightedProbe_0 _S104 = _S101;

#line 1330
    WeightedProbe_0 _S105 = lerp_probe_0(&_S103, &_S104, _S102);

#line 1330
    float3 _S106 = float3(2.09439516067504883f) ;

#line 1336
    return max(float3(dot(_S105.sh_0.sh_r_0.xyz / _S106, direction_2) + _S105.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S105.sh_0.sh_g_0.xyz / _S106, direction_2) + _S105.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S105.sh_0.sh_b_0.xyz / _S106, direction_2) + _S105.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S105.weight_1) , _S59);
}


#line 1353
float3 probe_environment_0(float3 world_position_5, float3 normal_4, float3 direction_3, KernelContext_0 thread* kernelContext_15)
{
    if((kernelContext_15->camera_0->probe_counts_0.w) == 0U)
    {

#line 1356
        return float3(0.0f, 0.0f, 0.0f);
    }

#line 1364
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_5, kernelContext_15->camera_0->probe_level_origin_0[int(0)].xyz, kernelContext_15->camera_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_15->camera_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_15->camera_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1366
    float3 _S107 = probe_level_environment_0(level_4, world_position_5, normal_4, direction_3, kernelContext_15);


    if(share_0 >= 1.0f)
    {

#line 1370
        return _S107;
    }

#line 1370
    float3 _S108 = probe_level_environment_0(level_4 + 1U, world_position_5, normal_4, direction_3, kernelContext_15);

    return _S108 * float3((1.0f - share_0))  + _S107 * float3(share_0) ;
}


#line 874
float2 decode_fixed_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 886
float2 fixed_pair_at_0(texture2d<float, access::sample> table_0, float2 at_2)
{
    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (table_0).get_width(0)),(*((&height_1)) = (table_0).get_height(0));
    float2 extent_6 = float2(float(width_1), float(height_1));
    float2 scaled_1 = saturate(at_2) * extent_6 - float2(0.5f) ;

#line 892
    float2 _S109 = float2(1.0f) ;
    float2 _S110 = extent_6 - _S109;

#line 893
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S110);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S111 = int2(low_1);
    int2 _S112 = int2(min(low_1 + _S109, _S110));
    int _S113 = _S111.x;

#line 899
    int _S114 = _S111.y;

#line 899
    int3 _S115 = int3(_S113, _S114, int(0));
    int _S116 = _S112.x;

#line 900
    int3 _S117 = int3(_S116, _S114, int(0));
    float2 _S118 = float2(weight_2.x) ;
    int _S119 = _S112.y;

#line 902
    int3 _S120 = int3(_S113, _S119, int(0));
    int3 _S121 = int3(_S116, _S119, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S115)).xy), uint(((_S115)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S117)).xy), uint(((_S117)).z)))), _S118), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S120)).xy), uint(((_S120)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S121)).xy), uint(((_S121)).z)))), _S118), float2(weight_2.y) );
}


float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_16)
{
    return fixed_pair_at_0(kernelContext_16->sky_prefilter_0, float2(up_0, roughness_1));
}


#line 932
float3 sky_prefiltered_0(float3 direction_4, float roughness_2, KernelContext_0 thread* kernelContext_17)
{
    float up_1 = clamp(direction_4.y, -1.0f, 1.0f);

#line 934
    float2 _S122 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_17);

    bool _S123 = up_1 >= 0.0f;

#line 936
    float3 far_0;

#line 936
    if(_S123)
    {

#line 936
        far_0 = kernelContext_17->camera_0->sky_0[int(0)].xyz;

#line 936
    }
    else
    {

#line 936
        far_0 = kernelContext_17->camera_0->sky_0[int(2)].xyz;

#line 936
    }

#line 936
    float3 opposite_0;
    if(_S123)
    {

#line 937
        opposite_0 = kernelContext_17->camera_0->sky_0[int(2)].xyz;

#line 937
    }
    else
    {

#line 937
        opposite_0 = kernelContext_17->camera_0->sky_0[int(0)].xyz;

#line 937
    }
    float _S124 = _S122.x;

#line 938
    float _S125 = _S122.y;
    return kernelContext_17->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S124 - _S125))  + far_0 * float3(_S124)  + opposite_0 * float3(_S125) ;
}


#line 951
float3 sky_view_at_0(float up_2, float azimuth_cosine_0, KernelContext_0 thread* kernelContext_18)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_0 = clamp(up_2, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_0));

#line 955
    float _S126;
    if(clamped_0 >= 0.0f)
    {

#line 956
        _S126 = root_0;

#line 956
    }
    else
    {

#line 956
        _S126 = - root_0;

#line 956
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    float x0_0 = clamp(floor(across_0), 0.0f, 95.0f);

    float fx_0 = clamp(across_0 - x0_0, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S126, 0.0f, 1.0f) * 64.0f - 0.5f;
    float y0_0 = clamp(floor(down_0), 0.0f, 63.0f);

    float fy_0 = clamp(down_0 - y0_0, 0.0f, 1.0f);

    uint row0_0 = uint(y0_0) * 96U;
    uint row1_0 = uint(min(y0_0 + 1.0f, 63.0f)) * 96U;
    uint _S127 = uint(x0_0);

#line 970
    float3 _S128 = float3((1.0f - fx_0)) ;
    uint _S129 = uint(min(x0_0 + 1.0f, 95.0f));

#line 971
    float3 _S130 = float3(fx_0) ;


    return ((float4(*(kernelContext_18->sky_view_0+(row0_0 + _S127))) ).xyz * _S128 + (float4(*(kernelContext_18->sky_view_0+(row0_0 + _S129))) ).xyz * _S130) * float3((1.0f - fy_0))  + ((float4(*(kernelContext_18->sky_view_0+(row1_0 + _S127))) ).xyz * _S128 + (float4(*(kernelContext_18->sky_view_0+(row1_0 + _S129))) ).xyz * _S130) * float3(fy_0) ;
}


#line 984
float3 atmosphere_radiance_0(float3 direction_5, KernelContext_0 thread* kernelContext_19)
{
    float3 sun_0 = kernelContext_19->camera_0->atmosphere_0.xyz;
    float _S131 = direction_5.x;

#line 987
    float _S132 = direction_5.z;

#line 987
    float view_flat_0 = sqrt(_S131 * _S131 + _S132 * _S132);
    float _S133 = sun_0.x;

#line 988
    float _S134 = sun_0.z;

#line 988
    float sun_flat_0 = sqrt(_S133 * _S133 + _S134 * _S134);

#line 988
    bool _S135;

    if(view_flat_0 > 0.0f)
    {

#line 990
        _S135 = sun_flat_0 > 0.0f;

#line 990
    }
    else
    {

#line 990
        _S135 = false;

#line 990
    }

#line 990
    float cosine_0;

#line 990
    if(_S135)
    {

#line 990
        cosine_0 = (_S131 * _S133 + _S132 * _S134) / (view_flat_0 * sun_flat_0);

#line 990
    }
    else
    {

#line 990
        cosine_0 = 1.0f;

#line 990
    }

#line 990
    float3 _S136 = sky_view_at_0(direction_5.y, cosine_0, kernelContext_19);



    return _S136;
}


#line 1024
float3 sky_environment_0(float3 direction_6, float roughness_3, float share_1, KernelContext_0 thread* kernelContext_20)
{

#line 1024
    float3 _S137 = sky_prefiltered_0(direction_6, roughness_3, kernelContext_20);


    if((kernelContext_20->camera_0->atmosphere_0.w) <= 0.0f)
    {
        return _S137;
    }



    float3 _S138 = _S137 * float3((1.0f - share_1)) ;

#line 1034
    float3 _S139 = atmosphere_radiance_0(direction_6, kernelContext_20);

#line 1034
    return _S138 + _S139 * float3(share_1) ;
}


#line 915
float2 dfg_at_0(float n_dot_v_0, float roughness_4, KernelContext_0 thread* kernelContext_21)
{
    return fixed_pair_at_0(kernelContext_21->dfg_0, float2(n_dot_v_0, roughness_4));
}


#line 718
float2 pixel_of_0(float2 ndc_1, float2 size_1)
{
    return float2((ndc_1.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_1.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_3, float2 size_2)
{
    return float2(at_3.x / size_2.x * 2.0f - 1.0f, 1.0f - at_3.y / size_2.y * 2.0f);
}


#line 814
float cell_exit_0(float2 at_4, float2 forward_0, float size_3, float reach_2)
{

    float _S140 = forward_0.x;

#line 817
    bool _S141 = _S140 > 0.0f;

#line 817
    float along_x_0;

#line 817
    if(_S141)
    {

#line 817
        along_x_0 = (floor(at_4.x / size_3) + 1.0f) * size_3;

#line 817
    }
    else
    {

#line 817
        along_x_0 = floor(at_4.x / size_3) * size_3;

#line 817
    }
    float _S142 = forward_0.y;

#line 818
    bool _S143 = _S142 > 0.0f;

#line 818
    float along_y_0;

#line 818
    if(_S143)
    {

#line 818
        along_y_0 = (floor(at_4.y / size_3) + 1.0f) * size_3;

#line 818
    }
    else
    {

#line 818
        along_y_0 = floor(at_4.y / size_3) * size_3;

#line 818
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 819
    float _S144;

    if((abs(_S140)) < 9.99999997475242708e-07f)
    {

#line 821
        along_x_0 = reach_2;

#line 821
    }
    else
    {

#line 822
        if(_S141)
        {

#line 822
            _S144 = nudge_0;

#line 822
        }
        else
        {

#line 822
            _S144 = - nudge_0;

#line 822
        }

#line 822
        along_x_0 = (along_x_0 + _S144 - at_4.x) / _S140;

#line 821
    }


    if((abs(_S142)) < 9.99999997475242708e-07f)
    {

#line 824
        along_y_0 = reach_2;

#line 824
    }
    else
    {

#line 825
        if(_S143)
        {

#line 825
            _S144 = nudge_0;

#line 825
        }
        else
        {

#line 825
            _S144 = - nudge_0;

#line 825
        }

#line 825
        along_y_0 = (along_y_0 + _S144 - at_4.y) / _S142;

#line 824
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 747
int2 hiz_clamp_0(int2 texel_1, int2 extent_7, uint level_5)
{
    return min(texel_1, max(int2((extent_7.x) >> level_5, (extent_7.y) >> level_5) - int2(int(1), int(1)), int2(int(0), int(0))));
}


#line 776
float hiz_at_0(uint level_6, int2 texel_2, int2 extent_8, KernelContext_0 thread* kernelContext_22)
{
    int2 _S145 = int2(int(0), int(0));
    int2 clamped_1 = clamp(texel_2, _S145, max(int2((extent_8.x) >> level_6, (extent_8.y) >> level_6) - int2(int(1), int(1)), _S145));
    int3 _S146 = int3(hiz_clamp_0(clamped_1, extent_8, 0U), int(0));

#line 780
    float d0_0 = ((kernelContext_22->scene_depth_0).read(vec<uint,2>(((_S146)).xy), uint(((_S146)).z)));
    int3 _S147 = int3(hiz_clamp_0(clamped_1, extent_8, 1U), int(0));

#line 781
    float d1_0 = ((kernelContext_22->hiz_1_0).read(vec<uint,2>(((_S147)).xy), uint(((_S147)).z)));
    int3 _S148 = int3(hiz_clamp_0(clamped_1, extent_8, 2U), int(0));

#line 782
    float d2_0 = ((kernelContext_22->hiz_2_0).read(vec<uint,2>(((_S148)).xy), uint(((_S148)).z)));
    int3 _S149 = int3(hiz_clamp_0(clamped_1, extent_8, 3U), int(0));

#line 783
    float d3_0 = ((kernelContext_22->hiz_3_0).read(vec<uint,2>(((_S149)).xy), uint(((_S149)).z)));
    int3 _S150 = int3(hiz_clamp_0(clamped_1, extent_8, 4U), int(0));

#line 784
    float d4_0 = ((kernelContext_22->hiz_4_0).read(vec<uint,2>(((_S150)).xy), uint(((_S150)).z)));
    int3 _S151 = int3(hiz_clamp_0(clamped_1, extent_8, 5U), int(0));

#line 785
    float d5_0 = ((kernelContext_22->hiz_5_0).read(vec<uint,2>(((_S151)).xy), uint(((_S151)).z)));

#line 785
    float _S152;
    if(level_6 == 0U)
    {

#line 786
        _S152 = d0_0;

#line 786
    }
    else
    {

#line 787
        if(level_6 == 1U)
        {

#line 787
            _S152 = d1_0;

#line 787
        }
        else
        {

#line 788
            if(level_6 == 2U)
            {

#line 788
                _S152 = d2_0;

#line 788
            }
            else
            {

#line 789
                if(level_6 == 3U)
                {

#line 789
                    _S152 = d3_0;

#line 789
                }
                else
                {

#line 790
                    if(level_6 == 4U)
                    {

#line 790
                        _S152 = d4_0;

#line 790
                    }
                    else
                    {

#line 790
                        _S152 = d5_0;

#line 790
                    }

#line 789
                }

#line 788
            }

#line 787
        }

#line 786
    }

#line 786
    return _S152;
}


#line 801
float view_z_of_0(float depth_5, KernelContext_0 thread* kernelContext_23)
{

#line 801
    float2 _S153 = unproject_z_1(depth_5, kernelContext_23);


    return _S153.x / _S153.y;
}


#line 737
float thickness_at_0(float advance_0, float depth_6)
{
    return max(advance_0, abs(depth_6) * 0.01999999955296516f);
}


#line 739
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 739
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 1387
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S154 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], packed_float4 device* sky_view_1 [[buffer(2)]], texture2d<float, access::sample> dfg_1 [[texture(9)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 1387
    float3 reflection_0;

#line 1387
    thread KernelContext_0 kernelContext_24;

#line 1387
    (&kernelContext_24)->scene_depth_0 = scene_depth_1;

#line 1387
    (&kernelContext_24)->camera_0 = camera_1;

#line 1387
    (&kernelContext_24)->reflectivity_0 = reflectivity_1;

#line 1387
    (&kernelContext_24)->probes_0 = probes_1;

#line 1387
    (&kernelContext_24)->probe_visibility_0 = probe_visibility_1;

#line 1387
    (&kernelContext_24)->sky_prefilter_0 = sky_prefilter_1;

#line 1387
    (&kernelContext_24)->sky_view_0 = sky_view_1;

#line 1387
    (&kernelContext_24)->dfg_0 = dfg_1;

#line 1387
    (&kernelContext_24)->hiz_1_0 = hiz_1_1;

#line 1387
    (&kernelContext_24)->hiz_2_0 = hiz_2_1;

#line 1387
    (&kernelContext_24)->hiz_3_0 = hiz_3_1;

#line 1387
    (&kernelContext_24)->hiz_4_0 = hiz_4_1;

#line 1387
    (&kernelContext_24)->hiz_5_0 = hiz_5_1;

#line 1387
    (&kernelContext_24)->scene_color_0 = scene_color_1;

    thread uint width_2;
    thread uint height_2;

#line 1397
    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    int2 extent_9 = int2(int(width_2), int(height_2));
    float _S155 = float(width_2);

#line 1399
    float _S156 = float(height_2);

#line 1399
    float2 size_4 = float2(_S155, _S156);

#line 1399
    int2 _S157 = representative_pixel_0(int2(position_0.xy), extent_9, int2(max(camera_1->hiz_0.yz, uint2(1U, 1U))), &kernelContext_24);

#line 1408
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S158 = int3(_S157, int(0));

#line 1410
    float4 surface_0 = (((&kernelContext_24)->reflectivity_0).read(vec<uint,2>(((_S158)).xy), uint(((_S158)).z)));
    float _S159 = surface_0.w;

#line 1411
    float sharpness_0 = sharpness_of_0(_S159);

#line 1411
    float _S160 = depth_at_0(_S157, extent_9, &kernelContext_24);


    if(_S160 <= 0.0f)
    {

#line 1414
        pixelOutput_0 _S161 = { NOTHING_0 };

        return _S161;
    }

#line 1416
    float3 _S162 = view_position_0(_S157, _S160, size_4, &kernelContext_24);

#line 1416
    float3 _S163 = normal_at_0(_S157, _S162, extent_9, size_4, &kernelContext_24);

#line 1422
    float3 towards_0 = normalize(_S162);
    float3 ray_0 = reflect(towards_0, _S163);


    float4 _S164 = float4(ray_0, 0.0f);

#line 1426
    float3 reflection_direction_0 = normalize((((_S164) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 1426
    float3 _S165 = probe_environment_0((((float4(_S162, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, normalize((((float4(_S163, 0.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz), reflection_direction_0, &kernelContext_24);

#line 1426
    float3 _S166 = sky_environment_0(reflection_direction_0, _S159, sharpness_0, &kernelContext_24);

#line 1449
    float3 environment_0 = _S165 + _S166;

#line 1457
    float3 _S167 = - towards_0;
    float3 f0_0 = surface_0.xyz;

#line 1458
    float2 _S168 = dfg_at_0(saturate(dot(_S163, _S167)), _S159, &kernelContext_24);

    float3 env_brdf_0 = f0_0 * float3(_S168.x)  + float3(_S168.y) ;

#line 1465
    if(sharpness_0 <= 0.0f)
    {

#line 1465
        pixelOutput_0 _S169 = { float4(environment_0 * env_brdf_0, 0.0f) };

        return _S169;
    }


    float _S170 = saturate((1.0f - dot(ray_0, _S167)) / 0.05000000074505806f);


    float _S171 = _S162.z;

#line 1474
    float3 start_0 = _S162 + _S163 * float3((abs(_S171) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S164) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S172 = clip_start_0.w;

#line 1479
    if(_S172 <= 0.0f)
    {

#line 1479
        pixelOutput_0 _S173 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S173;
    }
    float2 _S174 = clip_start_0.xy;

#line 1483
    float2 _S175 = float2(_S172) ;

#line 1483
    float2 at_start_0 = pixel_of_0(_S174 / _S175, size_4);

#line 1489
    float2 _S176 = clip_ray_0.xy;

#line 1489
    float _S177 = clip_ray_0.w;

#line 1489
    float2 _S178 = float2(_S177) ;

#line 1489
    float2 ndc_rate_0 = (_S176 * _S175 - _S174 * _S178) / float2((_S172 * _S172)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S155, - ndc_rate_0.y * 0.5f * _S156);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 1492
        pixelOutput_0 _S179 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S179;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 1503
    float reach_3 = 0.75f * min(_S155, _S156);

    float _S180 = forward_1.x;

#line 1505
    float travel_0;

#line 1505
    if(_S180 > 0.0f)
    {

#line 1505
        travel_0 = min(reach_3, (_S155 - 1.0f - at_start_0.x) / _S180);

#line 1505
    }
    else
    {

        if(_S180 < 0.0f)
        {

#line 1509
            travel_0 = min(reach_3, - at_start_0.x / _S180);

#line 1509
        }
        else
        {

#line 1509
            travel_0 = reach_3;

#line 1509
        }

#line 1505
    }

#line 1513
    float _S181 = forward_1.y;

#line 1513
    if(_S181 > 0.0f)
    {

#line 1513
        travel_0 = min(travel_0, (_S156 - 1.0f - at_start_0.y) / _S181);

#line 1513
    }
    else
    {

        if(_S181 < 0.0f)
        {

#line 1517
            travel_0 = min(travel_0, - at_start_0.y / _S181);

#line 1517
        }

#line 1513
    }

#line 1525
    if(_S177 > 0.0f)
    {

#line 1525
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S176 / _S178, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 1525
    }
    else
    {

#line 1540
        if(_S177 < 0.0f)
        {

#line 1547
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_24)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 1552
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S172) / _S177)) ;

#line 1552
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 1540
        }

#line 1525
    }

#line 1559
    float _S182 = max(travel_0, 0.0f);
    if(_S182 <= 0.00390625f)
    {

#line 1560
        pixelOutput_0 _S183 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S183;
    }

#line 1569
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S182) , size_4);

#line 1569
    float when_end_0;

    if((abs(_S180)) >= (abs(_S181)))
    {

#line 1571
        float _S184 = ndc_end_0.x;

#line 1571
        when_end_0 = (_S184 * _S172 - clip_start_0.x) / (clip_ray_0.x - _S184 * _S177);

#line 1571
    }
    else
    {

#line 1572
        float _S185 = ndc_end_0.y;

#line 1572
        when_end_0 = (_S185 * _S172 - clip_start_0.y) / (clip_ray_0.y - _S185 * _S177);

#line 1571
    }

#line 1571
    bool _S186;

#line 1579
    if(!(when_end_0 > 0.0f))
    {

#line 1579
        _S186 = true;

#line 1579
    }
    else
    {

#line 1579
        _S186 = !isfinite(when_end_0);

#line 1579
    }

#line 1579
    if(_S186)
    {

#line 1579
        pixelOutput_0 _S187 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S187;
    }

#line 1587
    float inverse_w_start_0 = 1.0f / _S172;

    float inverse_w_end_0 = 1.0f / (_S172 + when_end_0 * _S177);
    float _S188 = start_0.z;

#line 1590
    float _S189 = _S188 * inverse_w_start_0;
    float _S190 = (_S188 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 1596
    float3 _S191 = environment_0 * env_brdf_0;
    uint _S192 = min(camera_1->hiz_0.x, 5U);

#line 1627
    float _S193 = _S188 - _S171;

#line 1627
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S182), _S182);

#line 1627
    float previous_gap_0 = _S193;

#line 1627
    float entry_z_0 = _S188;

#line 1627
    uint step_0 = 0U;

#line 1627
    uint level_7 = 0U;

    for(;;)
    {

#line 1629
        if(step_0 < 96U)
        {
        }
        else
        {

#line 1629
            reflection_0 = _S191;

#line 1629
            break;
        }
        float cell_3 = float(1U << level_7);
        float2 at_5 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S194 = min(at_travel_0 + cell_exit_0(at_5, forward_1, cell_3, _S182), _S182);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S194) ;
        float along_0 = _S194 / _S182;

        float exit_z_0 = mix(_S189, _S190, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 1637
        float _S195 = hiz_at_0(level_7, int2(floor(at_5 / float2(cell_3) )), extent_9, &kernelContext_24);

#line 1637
        float gap_0;

#line 1644
        if(_S195 <= 0.0f)
        {

#line 1644
            gap_0 = 1.0f;

#line 1644
        }
        else
        {

#line 1644
            float _S196 = view_z_of_0(_S195, &kernelContext_24);

#line 1644
            gap_0 = exit_z_0 - _S196;

#line 1644
        }

#line 1653
        bool _S197 = !(gap_0 > 0.0f);

#line 1653
        if(_S197)
        {

#line 1653
            _S186 = level_7 > 0U;

#line 1653
        }
        else
        {

#line 1653
            _S186 = false;

#line 1653
        }

#line 1653
        if(_S186)
        {

#line 1653
            level_7 = level_7 - 1U;

#line 1659
            step_0 = step_0 + 1U;

#line 1629
            continue;
        }

#line 1629
        bool _S198;

#line 1662
        if(_S197)
        {

#line 1662
            _S198 = previous_gap_0 > 0.0f;

#line 1662
        }
        else
        {

#line 1662
            _S198 = false;

#line 1662
        }

#line 1662
        if(_S198)
        {



            float behind_1 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_1 <= thickness_0)
            {

#line 1675
                float2 hit_at_0 = mix(at_5, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1690
                float confidence_0 = sharpness_0 * _S170 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S194 / reach_3) / 0.25f) * saturate(1.0f - behind_1 / thickness_0);
                int3 _S199 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_9 - int2(int(1), int(1))), int(0));

#line 1691
                reflection_0 = (((&kernelContext_24)->scene_color_0).read(vec<uint,2>(((_S199)).xy), uint(((_S199)).z))).xyz * env_brdf_0 * float3(confidence_0)  + _S191 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 1662
        }

#line 1703
        if(_S194 >= _S182)
        {

#line 1703
            reflection_0 = _S191;

            break;
        }



        uint _S200 = min(level_7 + 1U, _S192);

#line 1710
        at_travel_0 = _S194;

#line 1710
        previous_gap_0 = gap_0;

#line 1710
        entry_z_0 = exit_z_0;

#line 1710
        level_7 = _S200;

#line 1629
        step_0 = step_0 + 1U;

#line 1629
    }

#line 1629
    pixelOutput_0 _S201 = { float4(reflection_0, sharpness_0) };

#line 1718
    return _S201;
}


#line 1718
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 544
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 544
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(10)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(8)]], packed_float4 device* sky_view_2 [[buffer(2)]], texture2d<float, access::sample> dfg_2 [[texture(9)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 544
    thread KernelContext_0 kernelContext_25;

#line 544
    (&kernelContext_25)->scene_depth_0 = scene_depth_2;

#line 544
    (&kernelContext_25)->camera_0 = camera_2;

#line 544
    (&kernelContext_25)->reflectivity_0 = reflectivity_2;

#line 544
    (&kernelContext_25)->probes_0 = probes_2;

#line 544
    (&kernelContext_25)->probe_visibility_0 = probe_visibility_2;

#line 544
    (&kernelContext_25)->sky_prefilter_0 = sky_prefilter_2;

#line 544
    (&kernelContext_25)->sky_view_0 = sky_view_2;

#line 544
    (&kernelContext_25)->dfg_0 = dfg_2;

#line 544
    (&kernelContext_25)->hiz_1_0 = hiz_1_2;

#line 544
    (&kernelContext_25)->hiz_2_0 = hiz_2_2;

#line 544
    (&kernelContext_25)->hiz_3_0 = hiz_3_2;

#line 544
    (&kernelContext_25)->hiz_4_0 = hiz_4_2;

#line 544
    (&kernelContext_25)->hiz_5_0 = hiz_5_2;

#line 544
    (&kernelContext_25)->scene_color_0 = scene_color_2;

#line 1378
    thread FullscreenOutput_0 output_1;


    float2 _S202 = float2(float((index_3 << 1U) & 2U), float(index_3 & 2U));

#line 1381
    (&output_1)->uv_2 = _S202;
    (&output_1)->position_2 = float4(_S202 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 1382
    thread vertexMain_Result_0 _S203;

#line 1382
    (&_S203)->position_1 = output_1.position_2;

#line 1382
    (&_S203)->uv_1 = output_1.uv_2;

#line 1382
    return _S203;
}

