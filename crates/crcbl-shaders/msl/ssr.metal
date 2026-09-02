#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 296 "shaders/ssr.slang"
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
    texture2d<float, access::sample> sky_prefilter_0;
    texture2d<float, access::sample> dfg_0;
    depth2d<float, access::sample> hiz_1_0;
    depth2d<float, access::sample> hiz_2_0;
    depth2d<float, access::sample> hiz_3_0;
    depth2d<float, access::sample> hiz_4_0;
    depth2d<float, access::sample> hiz_5_0;
    texture2d<float, access::sample> scene_color_0;
};


#line 442 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 445
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 442
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 445
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 463
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 463
float2 unproject_z_1(float depth_1, KernelContext_0 thread* kernelContext_3)
{
    return float2((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].z * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].w * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 494
float4 unproject_0(float2 ndc_0, float depth_2, KernelContext_0 thread* kernelContext_4)
{

#line 494
    float2 _S3 = unproject_z_0(depth_2, kernelContext_4);


    return float4((&kernelContext_4->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].y, _S3.x, _S3.y);
}


#line 510
float3 view_position_0(int2 pixel_2, float depth_3, float2 extent_2, KernelContext_0 thread* kernelContext_5)
{

#line 510
    float4 _S4 = unproject_0(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_3, kernelContext_5);

#line 521
    return _S4.xyz / float3(_S4.w) ;
}


#line 510
float3 view_position_1(int2 pixel_3, float depth_4, float2 extent_3, KernelContext_0 thread* kernelContext_6)
{

#line 510
    float4 _S5 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_4, kernelContext_6);

#line 521
    return _S5.xyz / float3(_S5.w) ;
}


#line 536
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_7)
{
    int2 _S6 = pixel_4 + int2(int(-1), int(0));

#line 538
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_7);

#line 538
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_7);
    int2 _S9 = pixel_4 + int2(int(1), int(0));

#line 539
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_7);

#line 539
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_7);
    int2 _S12 = pixel_4 + int2(int(0), int(-1));

#line 540
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_7);

#line 540
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_7);
    int2 _S15 = pixel_4 + int2(int(0), int(1));

#line 541
    float _S16 = depth_at_1(_S15, extent_4, kernelContext_7);

#line 541
    float3 _S17 = view_position_1(_S15, _S16, size_0, kernelContext_7);

    float _S18 = centre_0.z;

#line 543
    float3 horizontal_0;
    if((abs(_S11.z - _S18)) < (abs(_S18 - _S8.z)))
    {

#line 544
        horizontal_0 = _S11 - centre_0;

#line 544
    }
    else
    {

#line 544
        horizontal_0 = centre_0 - _S8;

#line 544
    }

#line 544
    float3 vertical_0;


    if((abs(_S17.z - _S18)) < (abs(_S18 - _S14.z)))
    {

#line 547
        vertical_0 = _S17 - centre_0;

#line 547
    }
    else
    {

#line 547
        vertical_0 = centre_0 - _S14;

#line 547
    }

#line 557
    return normalize(cross(vertical_0, horizontal_0));
}


#line 767
float probe_level_reach_0(float3 world_position_0, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 767
    float reach_0 = 0.0f;

#line 767
    uint axis_0 = 0U;


    for(;;)
    {

#line 770
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 770
            break;
        }

#line 770
        uint _S19 = axis_0;

#line 770
        bool _S20;

        if((last_0[axis_0]) == 0.0f)
        {

#line 772
            _S20 = true;

#line 772
        }
        else
        {

#line 772
            _S20 = (inv_spacing_0[axis_0]) == 0.0f;

#line 772
        }

#line 772
        if(_S20)
        {

#line 773
            axis_0 = axis_0 + 1U;

#line 770
            continue;
        }

#line 770
        reach_0 = max(reach_0, abs(2.0f * ((world_position_0[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S19] - 1.0f));

#line 770
        axis_0 = axis_0 + 1U;

#line 770
    }

#line 777
    return reach_0;
}


#line 787
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 787
    uint level_0 = 0U;

    for(;;)
    {

#line 789
        uint _S21 = level_0 + 1U;

#line 789
        if(_S21 < levels_0)
        {
        }
        else
        {

#line 789
            break;
        }
        float _S22 = float(level_0);

#line 791
        float at_0 = reach_1 * exp2(- _S22);
        if(at_0 < 1.0f)
        {

#line 793
            return float2(_S22, saturate((1.0f - at_0) / 0.25f));
        }

#line 789
        level_0 = _S21;

#line 789
    }

#line 795
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 152
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 806
float3 probe_level_environment_0(uint level_1, float3 world_position_1, float3 direction_0, KernelContext_0 thread* kernelContext_8)
{

#line 806
    float3 _S23 = float3(1.0f) ;

    float3 _S24 = float3(0.0f, 0.0f, 0.0f);

#line 808
    float3 last_1 = max(float3(kernelContext_8->camera_0->probe_counts_0.xyz) - _S23, _S24);

#line 814
    float3 grid_0 = clamp((world_position_1 - kernelContext_8->camera_0->probe_level_origin_0[level_1].xyz) * kernelContext_8->camera_0->probe_level_inv_spacing_0[level_1].xyz, _S24, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S25 = uint3(base_0);
    uint3 _S26 = uint3(min(base_0 + _S23, last_1));
    uint total_0 = max(kernelContext_8->camera_0->probe_counts_0.w, 1U) - 1U;


    uint first_0 = kernelContext_8->camera_0->probe_levels_0.y * level_1;
    uint _S27 = _S25.z;

#line 823
    uint _S28 = _S25.y;

#line 823
    uint _S29 = _S25.x;
    uint _S30 = _S26.x;
    uint _S31 = _S26.y;

    uint _S32 = _S26.z;



    GpuProbe_natural_0 x00_0 = kernelContext_8->probes_0[min(first_0 + (_S27 * kernelContext_8->camera_0->probe_counts_0.y + _S28) * kernelContext_8->camera_0->probe_counts_0.x + _S29, total_0)];
    GpuProbe_natural_0 x10_0 = kernelContext_8->probes_0[min(first_0 + (_S27 * kernelContext_8->camera_0->probe_counts_0.y + _S31) * kernelContext_8->camera_0->probe_counts_0.x + _S29, total_0)];
    GpuProbe_natural_0 x01_0 = kernelContext_8->probes_0[min(first_0 + (_S32 * kernelContext_8->camera_0->probe_counts_0.y + _S28) * kernelContext_8->camera_0->probe_counts_0.x + _S29, total_0)];
    GpuProbe_natural_0 x11_0 = kernelContext_8->probes_0[min(first_0 + (_S32 * kernelContext_8->camera_0->probe_counts_0.y + _S31) * kernelContext_8->camera_0->probe_counts_0.x + _S29, total_0)];
    GpuProbe_natural_0 y00_0 = kernelContext_8->probes_0[min(first_0 + (_S27 * kernelContext_8->camera_0->probe_counts_0.y + _S28) * kernelContext_8->camera_0->probe_counts_0.x + _S30, total_0)];
    GpuProbe_natural_0 y10_0 = kernelContext_8->probes_0[min(first_0 + (_S27 * kernelContext_8->camera_0->probe_counts_0.y + _S31) * kernelContext_8->camera_0->probe_counts_0.x + _S30, total_0)];
    GpuProbe_natural_0 y01_0 = kernelContext_8->probes_0[min(first_0 + (_S32 * kernelContext_8->camera_0->probe_counts_0.y + _S28) * kernelContext_8->camera_0->probe_counts_0.x + _S30, total_0)];
    GpuProbe_natural_0 y11_0 = kernelContext_8->probes_0[min(first_0 + (_S32 * kernelContext_8->camera_0->probe_counts_0.y + _S31) * kernelContext_8->camera_0->probe_counts_0.x + _S30, total_0)];
    thread GpuProbe_0 z0_0;
    float4 _S33 = float4(f_0.x) ;

#line 840
    float4 _S34 = float4(f_0.y) ;

#line 840
    float4 _S35 = mix(mix(float4(x00_0.sh_r_0) , float4(y00_0.sh_r_0) , _S33), mix(float4(x10_0.sh_r_0) , float4(y10_0.sh_r_0) , _S33), _S34);

#line 840
    (&z0_0)->sh_r_0 = _S35;
    float4 _S36 = mix(mix(float4(x00_0.sh_g_0) , float4(y00_0.sh_g_0) , _S33), mix(float4(x10_0.sh_g_0) , float4(y10_0.sh_g_0) , _S33), _S34);

#line 841
    (&z0_0)->sh_g_0 = _S36;
    float4 _S37 = mix(mix(float4(x00_0.sh_b_0) , float4(y00_0.sh_b_0) , _S33), mix(float4(x10_0.sh_b_0) , float4(y10_0.sh_b_0) , _S33), _S34);

#line 842
    (&z0_0)->sh_b_0 = _S37;
    thread GpuProbe_0 z1_0;
    float4 _S38 = mix(mix(float4(x01_0.sh_r_0) , float4(y01_0.sh_r_0) , _S33), mix(float4(x11_0.sh_r_0) , float4(y11_0.sh_r_0) , _S33), _S34);

#line 844
    (&z1_0)->sh_r_0 = _S38;
    float4 _S39 = mix(mix(float4(x01_0.sh_g_0) , float4(y01_0.sh_g_0) , _S33), mix(float4(x11_0.sh_g_0) , float4(y11_0.sh_g_0) , _S33), _S34);

#line 845
    (&z1_0)->sh_g_0 = _S39;
    float4 _S40 = mix(mix(float4(x01_0.sh_b_0) , float4(y01_0.sh_b_0) , _S33), mix(float4(x11_0.sh_b_0) , float4(y11_0.sh_b_0) , _S33), _S34);

#line 846
    (&z1_0)->sh_b_0 = _S40;
    thread GpuProbe_0 cell_0;
    float4 _S41 = float4(f_0.z) ;

#line 848
    float4 _S42 = mix(_S35, _S38, _S41);

#line 848
    (&cell_0)->sh_r_0 = _S42;
    float4 _S43 = mix(_S36, _S39, _S41);

#line 849
    (&cell_0)->sh_g_0 = _S43;
    float4 _S44 = mix(_S37, _S40, _S41);

#line 850
    (&cell_0)->sh_b_0 = _S44;

#line 850
    float3 _S45 = float3(2.09439516067504883f) ;
    return max(float3(dot(_S42.xyz / _S45, direction_0) + _S42.w / 3.14159274101257324f, dot(_S43.xyz / _S45, direction_0) + _S43.w / 3.14159274101257324f, dot(_S44.xyz / _S45, direction_0) + _S44.w / 3.14159274101257324f), _S24);
}


#line 867
float3 probe_environment_0(float3 world_position_2, float3 direction_1, KernelContext_0 thread* kernelContext_9)
{

#line 875
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_2, kernelContext_9->camera_0->probe_level_origin_0[int(0)].xyz, kernelContext_9->camera_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_9->camera_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_9->camera_0->probe_levels_0.x, 1U, 4U));
    uint level_2 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 877
    float3 _S46 = probe_level_environment_0(level_2, world_position_2, direction_1, kernelContext_9);


    if(share_0 >= 1.0f)
    {

#line 881
        return _S46;
    }

#line 881
    float3 _S47 = probe_level_environment_0(level_2 + 1U, world_position_2, direction_1, kernelContext_9);

    return _S47 * float3((1.0f - share_0))  + _S46 * float3(share_0) ;
}


#line 682
float2 decode_fixed_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 694
float2 fixed_pair_at_0(texture2d<float, access::sample> table_0, float2 at_1)
{
    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (table_0).get_width(0)),(*((&height_0)) = (table_0).get_height(0));
    float2 extent_5 = float2(float(width_0), float(height_0));
    float2 scaled_0 = saturate(at_1) * extent_5 - float2(0.5f) ;

#line 700
    float2 _S48 = float2(1.0f) ;
    float2 _S49 = extent_5 - _S48;

#line 701
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S49);

    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );

    int2 _S50 = int2(low_0);
    int2 _S51 = int2(min(low_0 + _S48, _S49));
    int _S52 = _S50.x;

#line 707
    int _S53 = _S50.y;

#line 707
    int3 _S54 = int3(_S52, _S53, int(0));
    int _S55 = _S51.x;

#line 708
    int3 _S56 = int3(_S55, _S53, int(0));
    float2 _S57 = float2(weight_0.x) ;
    int _S58 = _S51.y;

#line 710
    int3 _S59 = int3(_S52, _S58, int(0));
    int3 _S60 = int3(_S55, _S58, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S54)).xy), uint(((_S54)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S56)).xy), uint(((_S56)).z)))), _S57), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S59)).xy), uint(((_S59)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S60)).xy), uint(((_S60)).z)))), _S57), float2(weight_0.y) );
}


float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_10)
{
    return fixed_pair_at_0(kernelContext_10->sky_prefilter_0, float2(up_0, roughness_1));
}


#line 740
float3 sky_prefiltered_0(float3 direction_2, float roughness_2, KernelContext_0 thread* kernelContext_11)
{
    float up_1 = clamp(direction_2.y, -1.0f, 1.0f);

#line 742
    float2 _S61 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_11);

    bool _S62 = up_1 >= 0.0f;

#line 744
    float3 far_0;

#line 744
    if(_S62)
    {

#line 744
        far_0 = kernelContext_11->camera_0->sky_0[int(0)].xyz;

#line 744
    }
    else
    {

#line 744
        far_0 = kernelContext_11->camera_0->sky_0[int(2)].xyz;

#line 744
    }

#line 744
    float3 opposite_0;
    if(_S62)
    {

#line 745
        opposite_0 = kernelContext_11->camera_0->sky_0[int(2)].xyz;

#line 745
    }
    else
    {

#line 745
        opposite_0 = kernelContext_11->camera_0->sky_0[int(0)].xyz;

#line 745
    }
    float _S63 = _S61.x;

#line 746
    float _S64 = _S61.y;
    return kernelContext_11->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S63 - _S64))  + far_0 * float3(_S63)  + opposite_0 * float3(_S64) ;
}


#line 723
float2 dfg_at_0(float n_dot_v_0, float roughness_3, KernelContext_0 thread* kernelContext_12)
{
    return fixed_pair_at_0(kernelContext_12->dfg_0, float2(n_dot_v_0, roughness_3));
}


#line 566
float2 pixel_of_0(float2 ndc_1, float2 size_1)
{
    return float2((ndc_1.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_1.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_2, float2 size_2)
{
    return float2(at_2.x / size_2.x * 2.0f - 1.0f, 1.0f - at_2.y / size_2.y * 2.0f);
}


#line 643
float cell_exit_0(float2 at_3, float2 forward_0, float size_3, float reach_2)
{

    float _S65 = forward_0.x;

#line 646
    bool _S66 = _S65 > 0.0f;

#line 646
    float along_x_0;

#line 646
    if(_S66)
    {

#line 646
        along_x_0 = (floor(at_3.x / size_3) + 1.0f) * size_3;

#line 646
    }
    else
    {

#line 646
        along_x_0 = floor(at_3.x / size_3) * size_3;

#line 646
    }
    float _S67 = forward_0.y;

#line 647
    bool _S68 = _S67 > 0.0f;

#line 647
    float along_y_0;

#line 647
    if(_S68)
    {

#line 647
        along_y_0 = (floor(at_3.y / size_3) + 1.0f) * size_3;

#line 647
    }
    else
    {

#line 647
        along_y_0 = floor(at_3.y / size_3) * size_3;

#line 647
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 648
    float _S69;

    if((abs(_S65)) < 9.99999997475242708e-07f)
    {

#line 650
        along_x_0 = reach_2;

#line 650
    }
    else
    {

#line 651
        if(_S66)
        {

#line 651
            _S69 = nudge_0;

#line 651
        }
        else
        {

#line 651
            _S69 = - nudge_0;

#line 651
        }

#line 651
        along_x_0 = (along_x_0 + _S69 - at_3.x) / _S65;

#line 650
    }


    if((abs(_S67)) < 9.99999997475242708e-07f)
    {

#line 653
        along_y_0 = reach_2;

#line 653
    }
    else
    {

#line 654
        if(_S68)
        {

#line 654
            _S69 = nudge_0;

#line 654
        }
        else
        {

#line 654
            _S69 = - nudge_0;

#line 654
        }

#line 654
        along_y_0 = (along_y_0 + _S69 - at_3.y) / _S67;

#line 653
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 602
float hiz_at_0(uint level_3, int2 texel_1, int2 extent_6, KernelContext_0 thread* kernelContext_13)
{
    int2 _S70 = int2(int(0), int(0));
    int3 at_4 = int3(clamp(texel_1, _S70, max(extent_6 - int2(int(1), int(1)), _S70)), int(0));
    switch(level_3)
    {
    case 0U:
        {

#line 609
            return ((kernelContext_13->scene_depth_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 1U:
        {

#line 611
            return ((kernelContext_13->hiz_1_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 2U:
        {

#line 613
            return ((kernelContext_13->hiz_2_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 3U:
        {

#line 615
            return ((kernelContext_13->hiz_3_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    case 4U:
        {

#line 617
            return ((kernelContext_13->hiz_4_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    default:
        {

#line 619
            return ((kernelContext_13->hiz_5_0).read(vec<uint,2>(((at_4)).xy), uint(((at_4)).z)));
        }
    }

#line 619
}


#line 630
float view_z_of_0(float depth_5, KernelContext_0 thread* kernelContext_14)
{

#line 630
    float2 _S71 = unproject_z_1(depth_5, kernelContext_14);


    return _S71.x / _S71.y;
}


#line 585
float thickness_at_0(float advance_0, float depth_6)
{
    return max(advance_0, abs(depth_6) * 0.01999999955296516f);
}


#line 587
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 587
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 898
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S72 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], texture2d<float, access::sample> dfg_1 [[texture(9)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 898
    float3 reflection_0;

#line 898
    thread KernelContext_0 kernelContext_15;

#line 898
    (&kernelContext_15)->scene_depth_0 = scene_depth_1;

#line 898
    (&kernelContext_15)->reflectivity_0 = reflectivity_1;

#line 898
    (&kernelContext_15)->camera_0 = camera_1;

#line 898
    (&kernelContext_15)->probes_0 = probes_1;

#line 898
    (&kernelContext_15)->sky_prefilter_0 = sky_prefilter_1;

#line 898
    (&kernelContext_15)->dfg_0 = dfg_1;

#line 898
    (&kernelContext_15)->hiz_1_0 = hiz_1_1;

#line 898
    (&kernelContext_15)->hiz_2_0 = hiz_2_1;

#line 898
    (&kernelContext_15)->hiz_3_0 = hiz_3_1;

#line 898
    (&kernelContext_15)->hiz_4_0 = hiz_4_1;

#line 898
    (&kernelContext_15)->hiz_5_0 = hiz_5_1;

#line 898
    (&kernelContext_15)->scene_color_0 = scene_color_1;

    thread uint width_1;
    thread uint height_1;



    (*((&width_1)) = (scene_depth_1).get_width(0)),(*((&height_1)) = (scene_depth_1).get_height(0));
    int _S73 = int(width_1);

#line 906
    int _S74 = int(height_1);

#line 906
    int2 extent_7 = int2(_S73, _S74);
    float _S75 = float(width_1);

#line 907
    float _S76 = float(height_1);

#line 907
    float2 size_4 = float2(_S75, _S76);
    int2 _S77 = int2(position_0.xy);

#line 915
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S78 = int3(_S77, int(0));

#line 917
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S78)).xy), uint(((_S78)).z)));
    float _S79 = surface_0.w;

#line 918
    float sharpness_0 = sharpness_of_0(_S79);

#line 918
    float _S80 = depth_at_0(_S77, extent_7, &kernelContext_15);


    if(_S80 <= 0.0f)
    {

#line 921
        pixelOutput_0 _S81 = { NOTHING_0 };

        return _S81;
    }

#line 923
    float3 _S82 = view_position_0(_S77, _S80, size_4, &kernelContext_15);

#line 923
    float3 _S83 = normal_at_0(_S77, _S82, extent_7, size_4, &kernelContext_15);

#line 929
    float3 towards_0 = normalize(_S82);
    float3 ray_0 = reflect(towards_0, _S83);


    float4 _S84 = float4(ray_0, 0.0f);

#line 933
    float3 reflection_direction_0 = normalize((((_S84) * (matrix<float,int(4),int(4)> ((&kernelContext_15)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 933
    float3 _S85 = probe_environment_0((((float4(_S82, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_15)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_15)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, reflection_direction_0, &kernelContext_15);

#line 933
    float3 _S86 = sky_prefiltered_0(reflection_direction_0, _S79, &kernelContext_15);

#line 947
    float3 environment_0 = _S85 + _S86;

#line 955
    float3 _S87 = - towards_0;
    float3 f0_0 = surface_0.xyz;

#line 956
    float2 _S88 = dfg_at_0(saturate(dot(_S83, _S87)), _S79, &kernelContext_15);

    float3 env_brdf_0 = f0_0 * float3(_S88.x)  + float3(_S88.y) ;

#line 963
    if(sharpness_0 <= 0.0f)
    {

#line 963
        pixelOutput_0 _S89 = { float4(environment_0 * env_brdf_0, 0.0f) };

        return _S89;
    }


    float _S90 = saturate((1.0f - dot(ray_0, _S87)) / 0.05000000074505806f);


    float _S91 = _S82.z;

#line 972
    float3 start_0 = _S82 + _S83 * float3((abs(_S91) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_15)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_15)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_15)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_15)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_15)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_15)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_15)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_15)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_15)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_15)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_15)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_15)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_15)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_15)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_15)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_15)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S84) * (matrix<float,int(4),int(4)> ((&kernelContext_15)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_15)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_15)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_15)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_15)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_15)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_15)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_15)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_15)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_15)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_15)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_15)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_15)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_15)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_15)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_15)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S92 = clip_start_0.w;

#line 977
    if(_S92 <= 0.0f)
    {

#line 977
        pixelOutput_0 _S93 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S93;
    }
    float2 _S94 = clip_start_0.xy;

#line 981
    float2 _S95 = float2(_S92) ;

#line 981
    float2 at_start_0 = pixel_of_0(_S94 / _S95, size_4);

#line 987
    float2 _S96 = clip_ray_0.xy;

#line 987
    float _S97 = clip_ray_0.w;

#line 987
    float2 _S98 = float2(_S97) ;

#line 987
    float2 ndc_rate_0 = (_S96 * _S95 - _S94 * _S98) / float2((_S92 * _S92)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S75, - ndc_rate_0.y * 0.5f * _S76);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 990
        pixelOutput_0 _S99 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S99;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 1001
    float reach_3 = 0.75f * min(_S75, _S76);

    float _S100 = forward_1.x;

#line 1003
    float travel_0;

#line 1003
    if(_S100 > 0.0f)
    {

#line 1003
        travel_0 = min(reach_3, (_S75 - 1.0f - at_start_0.x) / _S100);

#line 1003
    }
    else
    {

        if(_S100 < 0.0f)
        {

#line 1007
            travel_0 = min(reach_3, - at_start_0.x / _S100);

#line 1007
        }
        else
        {

#line 1007
            travel_0 = reach_3;

#line 1007
        }

#line 1003
    }

#line 1011
    float _S101 = forward_1.y;

#line 1011
    if(_S101 > 0.0f)
    {

#line 1011
        travel_0 = min(travel_0, (_S76 - 1.0f - at_start_0.y) / _S101);

#line 1011
    }
    else
    {

        if(_S101 < 0.0f)
        {

#line 1015
            travel_0 = min(travel_0, - at_start_0.y / _S101);

#line 1015
        }

#line 1011
    }

#line 1023
    if(_S97 > 0.0f)
    {

#line 1023
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S96 / _S98, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 1023
    }
    else
    {

#line 1038
        if(_S97 < 0.0f)
        {

#line 1045
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_15)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_15)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 1050
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S92) / _S97)) ;

#line 1050
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 1038
        }

#line 1023
    }

#line 1057
    float _S102 = max(travel_0, 0.0f);
    if(_S102 <= 0.00390625f)
    {

#line 1058
        pixelOutput_0 _S103 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S103;
    }

#line 1067
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S102) , size_4);

#line 1067
    float when_end_0;

    if((abs(_S100)) >= (abs(_S101)))
    {

#line 1069
        float _S104 = ndc_end_0.x;

#line 1069
        when_end_0 = (_S104 * _S92 - clip_start_0.x) / (clip_ray_0.x - _S104 * _S97);

#line 1069
    }
    else
    {

#line 1070
        float _S105 = ndc_end_0.y;

#line 1070
        when_end_0 = (_S105 * _S92 - clip_start_0.y) / (clip_ray_0.y - _S105 * _S97);

#line 1069
    }

#line 1069
    bool _S106;

#line 1077
    if(!(when_end_0 > 0.0f))
    {

#line 1077
        _S106 = true;

#line 1077
    }
    else
    {

#line 1077
        _S106 = !isfinite(when_end_0);

#line 1077
    }

#line 1077
    if(_S106)
    {

#line 1077
        pixelOutput_0 _S107 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S107;
    }

#line 1085
    float inverse_w_start_0 = 1.0f / _S92;

    float inverse_w_end_0 = 1.0f / (_S92 + when_end_0 * _S97);
    float _S108 = start_0.z;

#line 1088
    float _S109 = _S108 * inverse_w_start_0;
    float _S110 = (_S108 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 1094
    float3 _S111 = environment_0 * env_brdf_0;
    uint _S112 = min((&kernelContext_15)->camera_0->hiz_0.x, 5U);

#line 1125
    float _S113 = _S108 - _S91;

#line 1125
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S102), _S102);

#line 1125
    float previous_gap_0 = _S113;

#line 1125
    float entry_z_0 = _S108;

#line 1125
    uint step_0 = 0U;

#line 1125
    uint level_4 = 0U;

    for(;;)
    {

#line 1127
        if(step_0 < 96U)
        {
        }
        else
        {

#line 1127
            reflection_0 = _S111;

#line 1127
            break;
        }
        float cell_1 = float(1U << level_4);
        float2 at_5 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S114 = min(at_travel_0 + cell_exit_0(at_5, forward_1, cell_1, _S102), _S102);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S114) ;
        float along_0 = _S114 / _S102;

        float exit_z_0 = mix(_S109, _S110, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 1135
        float _S115 = hiz_at_0(level_4, int2(floor(at_5 / float2(cell_1) )), int2(_S73 >> level_4, _S74 >> level_4), &kernelContext_15);

#line 1135
        float gap_0;

#line 1144
        if(_S115 <= 0.0f)
        {

#line 1144
            gap_0 = 1.0f;

#line 1144
        }
        else
        {

#line 1144
            float _S116 = view_z_of_0(_S115, &kernelContext_15);

#line 1144
            gap_0 = exit_z_0 - _S116;

#line 1144
        }

#line 1153
        bool _S117 = !(gap_0 > 0.0f);

#line 1153
        if(_S117)
        {

#line 1153
            _S106 = level_4 > 0U;

#line 1153
        }
        else
        {

#line 1153
            _S106 = false;

#line 1153
        }

#line 1153
        if(_S106)
        {

#line 1153
            level_4 = level_4 - 1U;

#line 1159
            step_0 = step_0 + 1U;

#line 1127
            continue;
        }

#line 1127
        bool _S118;

#line 1162
        if(_S117)
        {

#line 1162
            _S118 = previous_gap_0 > 0.0f;

#line 1162
        }
        else
        {

#line 1162
            _S118 = false;

#line 1162
        }

#line 1162
        if(_S118)
        {



            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {

#line 1175
                float2 hit_at_0 = mix(at_5, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1190
                float confidence_0 = sharpness_0 * _S90 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S114 / reach_3) / 0.25f) * saturate(1.0f - behind_0 / thickness_0);
                int3 _S119 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_7 - int2(int(1), int(1))), int(0));

#line 1191
                reflection_0 = (((&kernelContext_15)->scene_color_0).read(vec<uint,2>(((_S119)).xy), uint(((_S119)).z))).xyz * env_brdf_0 * float3(confidence_0)  + _S111 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 1162
        }

#line 1203
        if(_S114 >= _S102)
        {

#line 1203
            reflection_0 = _S111;

            break;
        }



        uint _S120 = min(level_4 + 1U, _S112);

#line 1210
        at_travel_0 = _S114;

#line 1210
        previous_gap_0 = gap_0;

#line 1210
        entry_z_0 = exit_z_0;

#line 1210
        level_4 = _S120;

#line 1127
        step_0 = step_0 + 1U;

#line 1127
    }

#line 1127
    pixelOutput_0 _S121 = { float4(reflection_0, sharpness_0) };

#line 1218
    return _S121;
}


#line 1218
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 430
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 430
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(8)]], texture2d<float, access::sample> dfg_2 [[texture(9)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 430
    thread KernelContext_0 kernelContext_16;

#line 430
    (&kernelContext_16)->scene_depth_0 = scene_depth_2;

#line 430
    (&kernelContext_16)->reflectivity_0 = reflectivity_2;

#line 430
    (&kernelContext_16)->camera_0 = camera_2;

#line 430
    (&kernelContext_16)->probes_0 = probes_2;

#line 430
    (&kernelContext_16)->sky_prefilter_0 = sky_prefilter_2;

#line 430
    (&kernelContext_16)->dfg_0 = dfg_2;

#line 430
    (&kernelContext_16)->hiz_1_0 = hiz_1_2;

#line 430
    (&kernelContext_16)->hiz_2_0 = hiz_2_2;

#line 430
    (&kernelContext_16)->hiz_3_0 = hiz_3_2;

#line 430
    (&kernelContext_16)->hiz_4_0 = hiz_4_2;

#line 430
    (&kernelContext_16)->hiz_5_0 = hiz_5_2;

#line 430
    (&kernelContext_16)->scene_color_0 = scene_color_2;

#line 889
    thread FullscreenOutput_0 output_1;


    float2 _S122 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 892
    (&output_1)->uv_2 = _S122;
    (&output_1)->position_2 = float4(_S122 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 893
    thread vertexMain_Result_0 _S123;

#line 893
    (&_S123)->position_1 = output_1.position_2;

#line 893
    (&_S123)->uv_1 = output_1.uv_2;

#line 893
    return _S123;
}

