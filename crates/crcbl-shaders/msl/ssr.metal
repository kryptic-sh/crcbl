#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 283 "shaders/ssr.slang"
float sharpness_of_0(float roughness_0)
{
    return saturate(1.0f - roughness_0 / 0.5f);
}


#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 98 "shaders/ssr.slang"
struct SsrParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    float4 probe_origin_0;
    float4 probe_inv_spacing_0;
    uint4 probe_counts_0;
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


#line 429 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 432
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 429
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 432
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 450
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_2)
{
    return float2((&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_2->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 450
float2 unproject_z_1(float depth_1, KernelContext_0 thread* kernelContext_3)
{
    return float2((&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].z * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(2)].w * depth_1 + (&kernelContext_3->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 481
float4 unproject_0(float2 ndc_0, float depth_2, KernelContext_0 thread* kernelContext_4)
{

#line 481
    float2 _S3 = unproject_z_0(depth_2, kernelContext_4);


    return float4((&kernelContext_4->camera_0->inv_proj_0)->data_0[int(0)].x * ndc_0.x + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].x, (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(1)].y * ndc_0.y + (&kernelContext_4->camera_0->inv_proj_0)->data_0[int(3)].y, _S3.x, _S3.y);
}


#line 497
float3 view_position_0(int2 pixel_2, float depth_3, float2 extent_2, KernelContext_0 thread* kernelContext_5)
{

#line 497
    float4 _S4 = unproject_0(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_3, kernelContext_5);

#line 508
    return _S4.xyz / float3(_S4.w) ;
}


#line 497
float3 view_position_1(int2 pixel_3, float depth_4, float2 extent_3, KernelContext_0 thread* kernelContext_6)
{

#line 497
    float4 _S5 = unproject_0(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_4, kernelContext_6);

#line 508
    return _S5.xyz / float3(_S5.w) ;
}


#line 523
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_7)
{
    int2 _S6 = pixel_4 + int2(int(-1), int(0));

#line 525
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_7);

#line 525
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_7);
    int2 _S9 = pixel_4 + int2(int(1), int(0));

#line 526
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_7);

#line 526
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_7);
    int2 _S12 = pixel_4 + int2(int(0), int(-1));

#line 527
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_7);

#line 527
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_7);
    int2 _S15 = pixel_4 + int2(int(0), int(1));

#line 528
    float _S16 = depth_at_1(_S15, extent_4, kernelContext_7);

#line 528
    float3 _S17 = view_position_1(_S15, _S16, size_0, kernelContext_7);

    float _S18 = centre_0.z;

#line 530
    float3 horizontal_0;
    if((abs(_S11.z - _S18)) < (abs(_S18 - _S8.z)))
    {

#line 531
        horizontal_0 = _S11 - centre_0;

#line 531
    }
    else
    {

#line 531
        horizontal_0 = centre_0 - _S8;

#line 531
    }

#line 531
    float3 vertical_0;


    if((abs(_S17.z - _S18)) < (abs(_S18 - _S14.z)))
    {

#line 534
        vertical_0 = _S17 - centre_0;

#line 534
    }
    else
    {

#line 534
        vertical_0 = centre_0 - _S14;

#line 534
    }

#line 544
    return normalize(cross(vertical_0, horizontal_0));
}


#line 139
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 736
float3 probe_environment_0(float3 world_position_0, float3 direction_0, KernelContext_0 thread* kernelContext_8)
{

#line 736
    float3 _S19 = float3(1.0f) ;

    float3 _S20 = float3(0.0f, 0.0f, 0.0f);

#line 738
    float3 last_0 = max(float3(kernelContext_8->camera_0->probe_counts_0.xyz) - _S19, _S20);
    float3 grid_0 = clamp((world_position_0 - kernelContext_8->camera_0->probe_origin_0.xyz) * kernelContext_8->camera_0->probe_inv_spacing_0.xyz, _S20, last_0);

    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S21 = uint3(base_0);
    uint3 _S22 = uint3(min(base_0 + _S19, last_0));
    uint total_0 = max(kernelContext_8->camera_0->probe_counts_0.w, 1U) - 1U;
    uint _S23 = _S21.z;

#line 746
    uint _S24 = _S21.y;

#line 746
    uint _S25 = _S21.x;
    uint _S26 = _S22.x;
    uint _S27 = _S22.y;

    uint _S28 = _S22.z;



    GpuProbe_natural_0 x00_0 = kernelContext_8->probes_0[min((_S23 * kernelContext_8->camera_0->probe_counts_0.y + _S24) * kernelContext_8->camera_0->probe_counts_0.x + _S25, total_0)];
    GpuProbe_natural_0 x10_0 = kernelContext_8->probes_0[min((_S23 * kernelContext_8->camera_0->probe_counts_0.y + _S27) * kernelContext_8->camera_0->probe_counts_0.x + _S25, total_0)];
    GpuProbe_natural_0 x01_0 = kernelContext_8->probes_0[min((_S28 * kernelContext_8->camera_0->probe_counts_0.y + _S24) * kernelContext_8->camera_0->probe_counts_0.x + _S25, total_0)];
    GpuProbe_natural_0 x11_0 = kernelContext_8->probes_0[min((_S28 * kernelContext_8->camera_0->probe_counts_0.y + _S27) * kernelContext_8->camera_0->probe_counts_0.x + _S25, total_0)];
    GpuProbe_natural_0 y00_0 = kernelContext_8->probes_0[min((_S23 * kernelContext_8->camera_0->probe_counts_0.y + _S24) * kernelContext_8->camera_0->probe_counts_0.x + _S26, total_0)];
    GpuProbe_natural_0 y10_0 = kernelContext_8->probes_0[min((_S23 * kernelContext_8->camera_0->probe_counts_0.y + _S27) * kernelContext_8->camera_0->probe_counts_0.x + _S26, total_0)];
    GpuProbe_natural_0 y01_0 = kernelContext_8->probes_0[min((_S28 * kernelContext_8->camera_0->probe_counts_0.y + _S24) * kernelContext_8->camera_0->probe_counts_0.x + _S26, total_0)];
    GpuProbe_natural_0 y11_0 = kernelContext_8->probes_0[min((_S28 * kernelContext_8->camera_0->probe_counts_0.y + _S27) * kernelContext_8->camera_0->probe_counts_0.x + _S26, total_0)];
    thread GpuProbe_0 z0_0;
    float4 _S29 = float4(f_0.x) ;

#line 763
    float4 _S30 = float4(f_0.y) ;

#line 763
    float4 _S31 = mix(mix(float4(x00_0.sh_r_0) , float4(y00_0.sh_r_0) , _S29), mix(float4(x10_0.sh_r_0) , float4(y10_0.sh_r_0) , _S29), _S30);

#line 763
    (&z0_0)->sh_r_0 = _S31;
    float4 _S32 = mix(mix(float4(x00_0.sh_g_0) , float4(y00_0.sh_g_0) , _S29), mix(float4(x10_0.sh_g_0) , float4(y10_0.sh_g_0) , _S29), _S30);

#line 764
    (&z0_0)->sh_g_0 = _S32;
    float4 _S33 = mix(mix(float4(x00_0.sh_b_0) , float4(y00_0.sh_b_0) , _S29), mix(float4(x10_0.sh_b_0) , float4(y10_0.sh_b_0) , _S29), _S30);

#line 765
    (&z0_0)->sh_b_0 = _S33;
    thread GpuProbe_0 z1_0;
    float4 _S34 = mix(mix(float4(x01_0.sh_r_0) , float4(y01_0.sh_r_0) , _S29), mix(float4(x11_0.sh_r_0) , float4(y11_0.sh_r_0) , _S29), _S30);

#line 767
    (&z1_0)->sh_r_0 = _S34;
    float4 _S35 = mix(mix(float4(x01_0.sh_g_0) , float4(y01_0.sh_g_0) , _S29), mix(float4(x11_0.sh_g_0) , float4(y11_0.sh_g_0) , _S29), _S30);

#line 768
    (&z1_0)->sh_g_0 = _S35;
    float4 _S36 = mix(mix(float4(x01_0.sh_b_0) , float4(y01_0.sh_b_0) , _S29), mix(float4(x11_0.sh_b_0) , float4(y11_0.sh_b_0) , _S29), _S30);

#line 769
    (&z1_0)->sh_b_0 = _S36;
    thread GpuProbe_0 cell_0;
    float4 _S37 = float4(f_0.z) ;

#line 771
    float4 _S38 = mix(_S31, _S34, _S37);

#line 771
    (&cell_0)->sh_r_0 = _S38;
    float4 _S39 = mix(_S32, _S35, _S37);

#line 772
    (&cell_0)->sh_g_0 = _S39;
    float4 _S40 = mix(_S33, _S36, _S37);

#line 773
    (&cell_0)->sh_b_0 = _S40;

#line 773
    float3 _S41 = float3(2.09439516067504883f) ;
    return max(float3(dot(_S38.xyz / _S41, direction_0) + _S38.w / 3.14159274101257324f, dot(_S39.xyz / _S41, direction_0) + _S39.w / 3.14159274101257324f, dot(_S40.xyz / _S41, direction_0) + _S40.w / 3.14159274101257324f), _S20);
}


#line 668
float2 decode_fixed_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 680
float2 fixed_pair_at_0(texture2d<float, access::sample> table_0, float2 at_0)
{
    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (table_0).get_width(0)),(*((&height_0)) = (table_0).get_height(0));
    float2 extent_5 = float2(float(width_0), float(height_0));
    float2 scaled_0 = saturate(at_0) * extent_5 - float2(0.5f) ;

#line 686
    float2 _S42 = float2(1.0f) ;
    float2 _S43 = extent_5 - _S42;

#line 687
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S43);

    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );

    int2 _S44 = int2(low_0);
    int2 _S45 = int2(min(low_0 + _S42, _S43));
    int _S46 = _S44.x;

#line 693
    int _S47 = _S44.y;

#line 693
    int3 _S48 = int3(_S46, _S47, int(0));
    int _S49 = _S45.x;

#line 694
    int3 _S50 = int3(_S49, _S47, int(0));
    float2 _S51 = float2(weight_0.x) ;
    int _S52 = _S45.y;

#line 696
    int3 _S53 = int3(_S46, _S52, int(0));
    int3 _S54 = int3(_S49, _S52, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S48)).xy), uint(((_S48)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)))), _S51), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S53)).xy), uint(((_S53)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S54)).xy), uint(((_S54)).z)))), _S51), float2(weight_0.y) );
}


float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_9)
{
    return fixed_pair_at_0(kernelContext_9->sky_prefilter_0, float2(up_0, roughness_1));
}


#line 726
float3 sky_prefiltered_0(float3 direction_1, float roughness_2, KernelContext_0 thread* kernelContext_10)
{
    float up_1 = clamp(direction_1.y, -1.0f, 1.0f);

#line 728
    float2 _S55 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_10);

    bool _S56 = up_1 >= 0.0f;

#line 730
    float3 far_0;

#line 730
    if(_S56)
    {

#line 730
        far_0 = kernelContext_10->camera_0->sky_0[int(0)].xyz;

#line 730
    }
    else
    {

#line 730
        far_0 = kernelContext_10->camera_0->sky_0[int(2)].xyz;

#line 730
    }

#line 730
    float3 opposite_0;
    if(_S56)
    {

#line 731
        opposite_0 = kernelContext_10->camera_0->sky_0[int(2)].xyz;

#line 731
    }
    else
    {

#line 731
        opposite_0 = kernelContext_10->camera_0->sky_0[int(0)].xyz;

#line 731
    }
    float _S57 = _S55.x;

#line 732
    float _S58 = _S55.y;
    return kernelContext_10->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S57 - _S58))  + far_0 * float3(_S57)  + opposite_0 * float3(_S58) ;
}


#line 709
float2 dfg_at_0(float n_dot_v_0, float roughness_3, KernelContext_0 thread* kernelContext_11)
{
    return fixed_pair_at_0(kernelContext_11->dfg_0, float2(n_dot_v_0, roughness_3));
}


#line 553
float2 pixel_of_0(float2 ndc_1, float2 size_1)
{
    return float2((ndc_1.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_1.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_1, float2 size_2)
{
    return float2(at_1.x / size_2.x * 2.0f - 1.0f, 1.0f - at_1.y / size_2.y * 2.0f);
}


#line 630
float cell_exit_0(float2 at_2, float2 forward_0, float size_3, float reach_0)
{

    float _S59 = forward_0.x;

#line 633
    bool _S60 = _S59 > 0.0f;

#line 633
    float along_x_0;

#line 633
    if(_S60)
    {

#line 633
        along_x_0 = (floor(at_2.x / size_3) + 1.0f) * size_3;

#line 633
    }
    else
    {

#line 633
        along_x_0 = floor(at_2.x / size_3) * size_3;

#line 633
    }
    float _S61 = forward_0.y;

#line 634
    bool _S62 = _S61 > 0.0f;

#line 634
    float along_y_0;

#line 634
    if(_S62)
    {

#line 634
        along_y_0 = (floor(at_2.y / size_3) + 1.0f) * size_3;

#line 634
    }
    else
    {

#line 634
        along_y_0 = floor(at_2.y / size_3) * size_3;

#line 634
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 635
    float _S63;

    if((abs(_S59)) < 9.99999997475242708e-07f)
    {

#line 637
        along_x_0 = reach_0;

#line 637
    }
    else
    {

#line 638
        if(_S60)
        {

#line 638
            _S63 = nudge_0;

#line 638
        }
        else
        {

#line 638
            _S63 = - nudge_0;

#line 638
        }

#line 638
        along_x_0 = (along_x_0 + _S63 - at_2.x) / _S59;

#line 637
    }


    if((abs(_S61)) < 9.99999997475242708e-07f)
    {

#line 640
        along_y_0 = reach_0;

#line 640
    }
    else
    {

#line 641
        if(_S62)
        {

#line 641
            _S63 = nudge_0;

#line 641
        }
        else
        {

#line 641
            _S63 = - nudge_0;

#line 641
        }

#line 641
        along_y_0 = (along_y_0 + _S63 - at_2.y) / _S61;

#line 640
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 589
float hiz_at_0(uint level_0, int2 texel_1, int2 extent_6, KernelContext_0 thread* kernelContext_12)
{
    int2 _S64 = int2(int(0), int(0));
    int3 at_3 = int3(clamp(texel_1, _S64, max(extent_6 - int2(int(1), int(1)), _S64)), int(0));
    switch(level_0)
    {
    case 0U:
        {

#line 596
            return ((kernelContext_12->scene_depth_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    case 1U:
        {

#line 598
            return ((kernelContext_12->hiz_1_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    case 2U:
        {

#line 600
            return ((kernelContext_12->hiz_2_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    case 3U:
        {

#line 602
            return ((kernelContext_12->hiz_3_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    case 4U:
        {

#line 604
            return ((kernelContext_12->hiz_4_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    default:
        {

#line 606
            return ((kernelContext_12->hiz_5_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    }

#line 606
}


#line 617
float view_z_of_0(float depth_5, KernelContext_0 thread* kernelContext_13)
{

#line 617
    float2 _S65 = unproject_z_1(depth_5, kernelContext_13);


    return _S65.x / _S65.y;
}


#line 572
float thickness_at_0(float advance_0, float depth_6)
{
    return max(advance_0, abs(depth_6) * 0.01999999955296516f);
}


#line 574
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 574
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 793
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S66 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], texture2d<float, access::sample> dfg_1 [[texture(9)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 793
    float3 reflection_0;

#line 793
    thread KernelContext_0 kernelContext_14;

#line 793
    (&kernelContext_14)->scene_depth_0 = scene_depth_1;

#line 793
    (&kernelContext_14)->reflectivity_0 = reflectivity_1;

#line 793
    (&kernelContext_14)->camera_0 = camera_1;

#line 793
    (&kernelContext_14)->probes_0 = probes_1;

#line 793
    (&kernelContext_14)->sky_prefilter_0 = sky_prefilter_1;

#line 793
    (&kernelContext_14)->dfg_0 = dfg_1;

#line 793
    (&kernelContext_14)->hiz_1_0 = hiz_1_1;

#line 793
    (&kernelContext_14)->hiz_2_0 = hiz_2_1;

#line 793
    (&kernelContext_14)->hiz_3_0 = hiz_3_1;

#line 793
    (&kernelContext_14)->hiz_4_0 = hiz_4_1;

#line 793
    (&kernelContext_14)->hiz_5_0 = hiz_5_1;

#line 793
    (&kernelContext_14)->scene_color_0 = scene_color_1;

    thread uint width_1;
    thread uint height_1;



    (*((&width_1)) = (scene_depth_1).get_width(0)),(*((&height_1)) = (scene_depth_1).get_height(0));
    int _S67 = int(width_1);

#line 801
    int _S68 = int(height_1);

#line 801
    int2 extent_7 = int2(_S67, _S68);
    float _S69 = float(width_1);

#line 802
    float _S70 = float(height_1);

#line 802
    float2 size_4 = float2(_S69, _S70);
    int2 _S71 = int2(position_0.xy);

#line 810
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S72 = int3(_S71, int(0));

#line 812
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S72)).xy), uint(((_S72)).z)));
    float _S73 = surface_0.w;

#line 813
    float sharpness_0 = sharpness_of_0(_S73);

#line 813
    float _S74 = depth_at_0(_S71, extent_7, &kernelContext_14);


    if(_S74 <= 0.0f)
    {

#line 816
        pixelOutput_0 _S75 = { NOTHING_0 };

        return _S75;
    }

#line 818
    float3 _S76 = view_position_0(_S71, _S74, size_4, &kernelContext_14);

#line 818
    float3 _S77 = normal_at_0(_S71, _S76, extent_7, size_4, &kernelContext_14);

#line 824
    float3 towards_0 = normalize(_S76);
    float3 ray_0 = reflect(towards_0, _S77);


    float4 _S78 = float4(ray_0, 0.0f);

#line 828
    float3 reflection_direction_0 = normalize((((_S78) * (matrix<float,int(4),int(4)> ((&kernelContext_14)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 828
    float3 _S79 = probe_environment_0((((float4(_S76, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_14)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_14)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, reflection_direction_0, &kernelContext_14);

#line 828
    float3 _S80 = sky_prefiltered_0(reflection_direction_0, _S73, &kernelContext_14);

#line 842
    float3 environment_0 = _S79 + _S80;

#line 850
    float3 _S81 = - towards_0;
    float3 f0_0 = surface_0.xyz;

#line 851
    float2 _S82 = dfg_at_0(saturate(dot(_S77, _S81)), _S73, &kernelContext_14);

    float3 env_brdf_0 = f0_0 * float3(_S82.x)  + float3(_S82.y) ;

#line 858
    if(sharpness_0 <= 0.0f)
    {

#line 858
        pixelOutput_0 _S83 = { float4(environment_0 * env_brdf_0, 0.0f) };

        return _S83;
    }


    float _S84 = saturate((1.0f - dot(ray_0, _S81)) / 0.05000000074505806f);


    float _S85 = _S76.z;

#line 867
    float3 start_0 = _S76 + _S77 * float3((abs(_S85) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_14)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_14)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_14)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_14)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_14)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_14)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_14)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_14)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_14)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_14)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_14)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_14)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_14)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_14)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_14)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_14)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S78) * (matrix<float,int(4),int(4)> ((&kernelContext_14)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_14)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_14)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_14)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_14)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_14)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_14)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_14)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_14)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_14)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_14)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_14)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_14)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_14)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_14)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_14)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S86 = clip_start_0.w;

#line 872
    if(_S86 <= 0.0f)
    {

#line 872
        pixelOutput_0 _S87 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S87;
    }
    float2 _S88 = clip_start_0.xy;

#line 876
    float2 _S89 = float2(_S86) ;

#line 876
    float2 at_start_0 = pixel_of_0(_S88 / _S89, size_4);

#line 882
    float2 _S90 = clip_ray_0.xy;

#line 882
    float _S91 = clip_ray_0.w;

#line 882
    float2 _S92 = float2(_S91) ;

#line 882
    float2 ndc_rate_0 = (_S90 * _S89 - _S88 * _S92) / float2((_S86 * _S86)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S69, - ndc_rate_0.y * 0.5f * _S70);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 885
        pixelOutput_0 _S93 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S93;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 896
    float reach_1 = 0.75f * min(_S69, _S70);

    float _S94 = forward_1.x;

#line 898
    float travel_0;

#line 898
    if(_S94 > 0.0f)
    {

#line 898
        travel_0 = min(reach_1, (_S69 - 1.0f - at_start_0.x) / _S94);

#line 898
    }
    else
    {

        if(_S94 < 0.0f)
        {

#line 902
            travel_0 = min(reach_1, - at_start_0.x / _S94);

#line 902
        }
        else
        {

#line 902
            travel_0 = reach_1;

#line 902
        }

#line 898
    }

#line 906
    float _S95 = forward_1.y;

#line 906
    if(_S95 > 0.0f)
    {

#line 906
        travel_0 = min(travel_0, (_S70 - 1.0f - at_start_0.y) / _S95);

#line 906
    }
    else
    {

        if(_S95 < 0.0f)
        {

#line 910
            travel_0 = min(travel_0, - at_start_0.y / _S95);

#line 910
        }

#line 906
    }

#line 918
    if(_S91 > 0.0f)
    {

#line 918
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S90 / _S92, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 918
    }
    else
    {

#line 933
        if(_S91 < 0.0f)
        {

#line 940
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_14)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_14)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 945
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S86) / _S91)) ;

#line 945
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 933
        }

#line 918
    }

#line 952
    float _S96 = max(travel_0, 0.0f);
    if(_S96 <= 0.00390625f)
    {

#line 953
        pixelOutput_0 _S97 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S97;
    }

#line 962
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S96) , size_4);

#line 962
    float when_end_0;

    if((abs(_S94)) >= (abs(_S95)))
    {

#line 964
        float _S98 = ndc_end_0.x;

#line 964
        when_end_0 = (_S98 * _S86 - clip_start_0.x) / (clip_ray_0.x - _S98 * _S91);

#line 964
    }
    else
    {

#line 965
        float _S99 = ndc_end_0.y;

#line 965
        when_end_0 = (_S99 * _S86 - clip_start_0.y) / (clip_ray_0.y - _S99 * _S91);

#line 964
    }

#line 964
    bool _S100;

#line 972
    if(!(when_end_0 > 0.0f))
    {

#line 972
        _S100 = true;

#line 972
    }
    else
    {

#line 972
        _S100 = !isfinite(when_end_0);

#line 972
    }

#line 972
    if(_S100)
    {

#line 972
        pixelOutput_0 _S101 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S101;
    }

#line 980
    float inverse_w_start_0 = 1.0f / _S86;

    float inverse_w_end_0 = 1.0f / (_S86 + when_end_0 * _S91);
    float _S102 = start_0.z;

#line 983
    float _S103 = _S102 * inverse_w_start_0;
    float _S104 = (_S102 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 989
    float3 _S105 = environment_0 * env_brdf_0;
    uint _S106 = min((&kernelContext_14)->camera_0->hiz_0.x, 5U);

#line 1020
    float _S107 = _S102 - _S85;

#line 1020
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S96), _S96);

#line 1020
    float previous_gap_0 = _S107;

#line 1020
    float entry_z_0 = _S102;

#line 1020
    uint step_0 = 0U;

#line 1020
    uint level_1 = 0U;

    for(;;)
    {

#line 1022
        if(step_0 < 96U)
        {
        }
        else
        {

#line 1022
            reflection_0 = _S105;

#line 1022
            break;
        }
        float cell_1 = float(1U << level_1);
        float2 at_4 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S108 = min(at_travel_0 + cell_exit_0(at_4, forward_1, cell_1, _S96), _S96);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S108) ;
        float along_0 = _S108 / _S96;

        float exit_z_0 = mix(_S103, _S104, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 1030
        float _S109 = hiz_at_0(level_1, int2(floor(at_4 / float2(cell_1) )), int2(_S67 >> level_1, _S68 >> level_1), &kernelContext_14);

#line 1030
        float gap_0;

#line 1039
        if(_S109 <= 0.0f)
        {

#line 1039
            gap_0 = 1.0f;

#line 1039
        }
        else
        {

#line 1039
            float _S110 = view_z_of_0(_S109, &kernelContext_14);

#line 1039
            gap_0 = exit_z_0 - _S110;

#line 1039
        }

#line 1048
        bool _S111 = !(gap_0 > 0.0f);

#line 1048
        if(_S111)
        {

#line 1048
            _S100 = level_1 > 0U;

#line 1048
        }
        else
        {

#line 1048
            _S100 = false;

#line 1048
        }

#line 1048
        if(_S100)
        {

#line 1048
            level_1 = level_1 - 1U;

#line 1054
            step_0 = step_0 + 1U;

#line 1022
            continue;
        }

#line 1022
        bool _S112;

#line 1057
        if(_S111)
        {

#line 1057
            _S112 = previous_gap_0 > 0.0f;

#line 1057
        }
        else
        {

#line 1057
            _S112 = false;

#line 1057
        }

#line 1057
        if(_S112)
        {



            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {

#line 1070
                float2 hit_at_0 = mix(at_4, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1085
                float confidence_0 = sharpness_0 * _S84 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S108 / reach_1) / 0.25f) * saturate(1.0f - behind_0 / thickness_0);
                int3 _S113 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_7 - int2(int(1), int(1))), int(0));

#line 1086
                reflection_0 = (((&kernelContext_14)->scene_color_0).read(vec<uint,2>(((_S113)).xy), uint(((_S113)).z))).xyz * env_brdf_0 * float3(confidence_0)  + _S105 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 1057
        }

#line 1098
        if(_S108 >= _S96)
        {

#line 1098
            reflection_0 = _S105;

            break;
        }



        uint _S114 = min(level_1 + 1U, _S106);

#line 1105
        at_travel_0 = _S108;

#line 1105
        previous_gap_0 = gap_0;

#line 1105
        entry_z_0 = exit_z_0;

#line 1105
        level_1 = _S114;

#line 1022
        step_0 = step_0 + 1U;

#line 1022
    }

#line 1022
    pixelOutput_0 _S115 = { float4(reflection_0, sharpness_0) };

#line 1113
    return _S115;
}


#line 1113
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 417
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 417
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(8)]], texture2d<float, access::sample> dfg_2 [[texture(9)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 417
    thread KernelContext_0 kernelContext_15;

#line 417
    (&kernelContext_15)->scene_depth_0 = scene_depth_2;

#line 417
    (&kernelContext_15)->reflectivity_0 = reflectivity_2;

#line 417
    (&kernelContext_15)->camera_0 = camera_2;

#line 417
    (&kernelContext_15)->probes_0 = probes_2;

#line 417
    (&kernelContext_15)->sky_prefilter_0 = sky_prefilter_2;

#line 417
    (&kernelContext_15)->dfg_0 = dfg_2;

#line 417
    (&kernelContext_15)->hiz_1_0 = hiz_1_2;

#line 417
    (&kernelContext_15)->hiz_2_0 = hiz_2_2;

#line 417
    (&kernelContext_15)->hiz_3_0 = hiz_3_2;

#line 417
    (&kernelContext_15)->hiz_4_0 = hiz_4_2;

#line 417
    (&kernelContext_15)->hiz_5_0 = hiz_5_2;

#line 417
    (&kernelContext_15)->scene_color_0 = scene_color_2;

#line 784
    thread FullscreenOutput_0 output_1;


    float2 _S116 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 787
    (&output_1)->uv_2 = _S116;
    (&output_1)->position_2 = float4(_S116 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 788
    thread vertexMain_Result_0 _S117;

#line 788
    (&_S117)->position_1 = output_1.position_2;

#line 788
    (&_S117)->uv_1 = output_1.uv_2;

#line 788
    return _S117;
}

