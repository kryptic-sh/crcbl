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


#line 441
float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 451
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 441
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 451
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 467
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 469
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 469
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 470
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 470
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 471
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 471
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 472
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 472
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 474
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 475
        horizontal_0 = _S8 - centre_0;

#line 475
    }
    else
    {

#line 475
        horizontal_0 = centre_0 - _S5;

#line 475
    }

#line 475
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 478
        vertical_0 = _S14 - centre_0;

#line 478
    }
    else
    {

#line 478
        vertical_0 = centre_0 - _S11;

#line 478
    }

#line 488
    return normalize(cross(vertical_0, horizontal_0));
}


#line 139
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 680
float3 probe_environment_0(float3 world_position_0, float3 direction_0, KernelContext_0 thread* kernelContext_5)
{

#line 680
    float3 _S16 = float3(1.0f) ;

    float3 _S17 = float3(0.0f, 0.0f, 0.0f);

#line 682
    float3 last_0 = max(float3(kernelContext_5->camera_0->probe_counts_0.xyz) - _S16, _S17);
    float3 grid_0 = clamp((world_position_0 - kernelContext_5->camera_0->probe_origin_0.xyz) * kernelContext_5->camera_0->probe_inv_spacing_0.xyz, _S17, last_0);

    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S18 = uint3(base_0);
    uint3 _S19 = uint3(min(base_0 + _S16, last_0));
    uint total_0 = max(kernelContext_5->camera_0->probe_counts_0.w, 1U) - 1U;
    uint _S20 = _S18.z;

#line 690
    uint _S21 = _S18.y;

#line 690
    uint _S22 = _S18.x;
    uint _S23 = _S19.x;
    uint _S24 = _S19.y;

    uint _S25 = _S19.z;



    GpuProbe_natural_0 x00_0 = kernelContext_5->probes_0[min((_S20 * kernelContext_5->camera_0->probe_counts_0.y + _S21) * kernelContext_5->camera_0->probe_counts_0.x + _S22, total_0)];
    GpuProbe_natural_0 x10_0 = kernelContext_5->probes_0[min((_S20 * kernelContext_5->camera_0->probe_counts_0.y + _S24) * kernelContext_5->camera_0->probe_counts_0.x + _S22, total_0)];
    GpuProbe_natural_0 x01_0 = kernelContext_5->probes_0[min((_S25 * kernelContext_5->camera_0->probe_counts_0.y + _S21) * kernelContext_5->camera_0->probe_counts_0.x + _S22, total_0)];
    GpuProbe_natural_0 x11_0 = kernelContext_5->probes_0[min((_S25 * kernelContext_5->camera_0->probe_counts_0.y + _S24) * kernelContext_5->camera_0->probe_counts_0.x + _S22, total_0)];
    GpuProbe_natural_0 y00_0 = kernelContext_5->probes_0[min((_S20 * kernelContext_5->camera_0->probe_counts_0.y + _S21) * kernelContext_5->camera_0->probe_counts_0.x + _S23, total_0)];
    GpuProbe_natural_0 y10_0 = kernelContext_5->probes_0[min((_S20 * kernelContext_5->camera_0->probe_counts_0.y + _S24) * kernelContext_5->camera_0->probe_counts_0.x + _S23, total_0)];
    GpuProbe_natural_0 y01_0 = kernelContext_5->probes_0[min((_S25 * kernelContext_5->camera_0->probe_counts_0.y + _S21) * kernelContext_5->camera_0->probe_counts_0.x + _S23, total_0)];
    GpuProbe_natural_0 y11_0 = kernelContext_5->probes_0[min((_S25 * kernelContext_5->camera_0->probe_counts_0.y + _S24) * kernelContext_5->camera_0->probe_counts_0.x + _S23, total_0)];
    thread GpuProbe_0 z0_0;
    float4 _S26 = float4(f_0.x) ;

#line 707
    float4 _S27 = float4(f_0.y) ;

#line 707
    float4 _S28 = mix(mix(float4(x00_0.sh_r_0) , float4(y00_0.sh_r_0) , _S26), mix(float4(x10_0.sh_r_0) , float4(y10_0.sh_r_0) , _S26), _S27);

#line 707
    (&z0_0)->sh_r_0 = _S28;
    float4 _S29 = mix(mix(float4(x00_0.sh_g_0) , float4(y00_0.sh_g_0) , _S26), mix(float4(x10_0.sh_g_0) , float4(y10_0.sh_g_0) , _S26), _S27);

#line 708
    (&z0_0)->sh_g_0 = _S29;
    float4 _S30 = mix(mix(float4(x00_0.sh_b_0) , float4(y00_0.sh_b_0) , _S26), mix(float4(x10_0.sh_b_0) , float4(y10_0.sh_b_0) , _S26), _S27);

#line 709
    (&z0_0)->sh_b_0 = _S30;
    thread GpuProbe_0 z1_0;
    float4 _S31 = mix(mix(float4(x01_0.sh_r_0) , float4(y01_0.sh_r_0) , _S26), mix(float4(x11_0.sh_r_0) , float4(y11_0.sh_r_0) , _S26), _S27);

#line 711
    (&z1_0)->sh_r_0 = _S31;
    float4 _S32 = mix(mix(float4(x01_0.sh_g_0) , float4(y01_0.sh_g_0) , _S26), mix(float4(x11_0.sh_g_0) , float4(y11_0.sh_g_0) , _S26), _S27);

#line 712
    (&z1_0)->sh_g_0 = _S32;
    float4 _S33 = mix(mix(float4(x01_0.sh_b_0) , float4(y01_0.sh_b_0) , _S26), mix(float4(x11_0.sh_b_0) , float4(y11_0.sh_b_0) , _S26), _S27);

#line 713
    (&z1_0)->sh_b_0 = _S33;
    thread GpuProbe_0 cell_0;
    float4 _S34 = float4(f_0.z) ;

#line 715
    float4 _S35 = mix(_S28, _S31, _S34);

#line 715
    (&cell_0)->sh_r_0 = _S35;
    float4 _S36 = mix(_S29, _S32, _S34);

#line 716
    (&cell_0)->sh_g_0 = _S36;
    float4 _S37 = mix(_S30, _S33, _S34);

#line 717
    (&cell_0)->sh_b_0 = _S37;

#line 717
    float3 _S38 = float3(2.09439516067504883f) ;
    return max(float3(dot(_S35.xyz / _S38, direction_0) + _S35.w / 3.14159274101257324f, dot(_S36.xyz / _S38, direction_0) + _S36.w / 3.14159274101257324f, dot(_S37.xyz / _S38, direction_0) + _S37.w / 3.14159274101257324f), _S17);
}


#line 612
float2 decode_fixed_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 624
float2 fixed_pair_at_0(texture2d<float, access::sample> table_0, float2 at_0)
{
    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (table_0).get_width(0)),(*((&height_0)) = (table_0).get_height(0));
    float2 extent_5 = float2(float(width_0), float(height_0));
    float2 scaled_0 = saturate(at_0) * extent_5 - float2(0.5f) ;

#line 630
    float2 _S39 = float2(1.0f) ;
    float2 _S40 = extent_5 - _S39;

#line 631
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S40);

    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );

    int2 _S41 = int2(low_0);
    int2 _S42 = int2(min(low_0 + _S39, _S40));
    int _S43 = _S41.x;

#line 637
    int _S44 = _S41.y;

#line 637
    int3 _S45 = int3(_S43, _S44, int(0));
    int _S46 = _S42.x;

#line 638
    int3 _S47 = int3(_S46, _S44, int(0));
    float2 _S48 = float2(weight_0.x) ;
    int _S49 = _S42.y;

#line 640
    int3 _S50 = int3(_S43, _S49, int(0));
    int3 _S51 = int3(_S46, _S49, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S45)).xy), uint(((_S45)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S47)).xy), uint(((_S47)).z)))), _S48), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S51)).xy), uint(((_S51)).z)))), _S48), float2(weight_0.y) );
}


float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_6)
{
    return fixed_pair_at_0(kernelContext_6->sky_prefilter_0, float2(up_0, roughness_1));
}


#line 670
float3 sky_prefiltered_0(float3 direction_1, float roughness_2, KernelContext_0 thread* kernelContext_7)
{
    float up_1 = clamp(direction_1.y, -1.0f, 1.0f);

#line 672
    float2 _S52 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_7);

    bool _S53 = up_1 >= 0.0f;

#line 674
    float3 far_0;

#line 674
    if(_S53)
    {

#line 674
        far_0 = kernelContext_7->camera_0->sky_0[int(0)].xyz;

#line 674
    }
    else
    {

#line 674
        far_0 = kernelContext_7->camera_0->sky_0[int(2)].xyz;

#line 674
    }

#line 674
    float3 opposite_0;
    if(_S53)
    {

#line 675
        opposite_0 = kernelContext_7->camera_0->sky_0[int(2)].xyz;

#line 675
    }
    else
    {

#line 675
        opposite_0 = kernelContext_7->camera_0->sky_0[int(0)].xyz;

#line 675
    }
    float _S54 = _S52.x;

#line 676
    float _S55 = _S52.y;
    return kernelContext_7->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S54 - _S55))  + far_0 * float3(_S54)  + opposite_0 * float3(_S55) ;
}


#line 653
float2 dfg_at_0(float n_dot_v_0, float roughness_3, KernelContext_0 thread* kernelContext_8)
{
    return fixed_pair_at_0(kernelContext_8->dfg_0, float2(n_dot_v_0, roughness_3));
}


#line 497
float2 pixel_of_0(float2 ndc_0, float2 size_1)
{
    return float2((ndc_0.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_0.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_1, float2 size_2)
{
    return float2(at_1.x / size_2.x * 2.0f - 1.0f, 1.0f - at_1.y / size_2.y * 2.0f);
}


#line 574
float cell_exit_0(float2 at_2, float2 forward_0, float size_3, float reach_0)
{

    float _S56 = forward_0.x;

#line 577
    bool _S57 = _S56 > 0.0f;

#line 577
    float along_x_0;

#line 577
    if(_S57)
    {

#line 577
        along_x_0 = (floor(at_2.x / size_3) + 1.0f) * size_3;

#line 577
    }
    else
    {

#line 577
        along_x_0 = floor(at_2.x / size_3) * size_3;

#line 577
    }
    float _S58 = forward_0.y;

#line 578
    bool _S59 = _S58 > 0.0f;

#line 578
    float along_y_0;

#line 578
    if(_S59)
    {

#line 578
        along_y_0 = (floor(at_2.y / size_3) + 1.0f) * size_3;

#line 578
    }
    else
    {

#line 578
        along_y_0 = floor(at_2.y / size_3) * size_3;

#line 578
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 579
    float _S60;

    if((abs(_S56)) < 9.99999997475242708e-07f)
    {

#line 581
        along_x_0 = reach_0;

#line 581
    }
    else
    {

#line 582
        if(_S57)
        {

#line 582
            _S60 = nudge_0;

#line 582
        }
        else
        {

#line 582
            _S60 = - nudge_0;

#line 582
        }

#line 582
        along_x_0 = (along_x_0 + _S60 - at_2.x) / _S56;

#line 581
    }


    if((abs(_S58)) < 9.99999997475242708e-07f)
    {

#line 584
        along_y_0 = reach_0;

#line 584
    }
    else
    {

#line 585
        if(_S59)
        {

#line 585
            _S60 = nudge_0;

#line 585
        }
        else
        {

#line 585
            _S60 = - nudge_0;

#line 585
        }

#line 585
        along_y_0 = (along_y_0 + _S60 - at_2.y) / _S58;

#line 584
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 533
float hiz_at_0(uint level_0, int2 texel_1, int2 extent_6, KernelContext_0 thread* kernelContext_9)
{
    int2 _S61 = int2(int(0), int(0));
    int3 at_3 = int3(clamp(texel_1, _S61, max(extent_6 - int2(int(1), int(1)), _S61)), int(0));
    switch(level_0)
    {
    case 0U:
        {

#line 540
            return ((kernelContext_9->scene_depth_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    case 1U:
        {

#line 542
            return ((kernelContext_9->hiz_1_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    case 2U:
        {

#line 544
            return ((kernelContext_9->hiz_2_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    case 3U:
        {

#line 546
            return ((kernelContext_9->hiz_3_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    case 4U:
        {

#line 548
            return ((kernelContext_9->hiz_4_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    default:
        {

#line 550
            return ((kernelContext_9->hiz_5_0).read(vec<uint,2>(((at_3)).xy), uint(((at_3)).z)));
        }
    }

#line 550
}


#line 561
float view_z_of_0(float depth_2, KernelContext_0 thread* kernelContext_10)
{
    float4 view_2 = (((float4(0.0f, 0.0f, depth_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_10->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_10->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_10->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_10->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_10->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_10->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_10->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_10->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_10->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_10->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_10->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_10->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_10->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_10->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_10->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_10->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_2.z / view_2.w;
}


#line 516
float thickness_at_0(float advance_0, float depth_3)
{
    return max(advance_0, abs(depth_3) * 0.01999999955296516f);
}


#line 518
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 518
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 737
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S62 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], texture2d<float, access::sample> dfg_1 [[texture(9)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 737
    float3 reflection_0;

#line 737
    thread KernelContext_0 kernelContext_11;

#line 737
    (&kernelContext_11)->scene_depth_0 = scene_depth_1;

#line 737
    (&kernelContext_11)->reflectivity_0 = reflectivity_1;

#line 737
    (&kernelContext_11)->camera_0 = camera_1;

#line 737
    (&kernelContext_11)->probes_0 = probes_1;

#line 737
    (&kernelContext_11)->sky_prefilter_0 = sky_prefilter_1;

#line 737
    (&kernelContext_11)->dfg_0 = dfg_1;

#line 737
    (&kernelContext_11)->hiz_1_0 = hiz_1_1;

#line 737
    (&kernelContext_11)->hiz_2_0 = hiz_2_1;

#line 737
    (&kernelContext_11)->hiz_3_0 = hiz_3_1;

#line 737
    (&kernelContext_11)->hiz_4_0 = hiz_4_1;

#line 737
    (&kernelContext_11)->hiz_5_0 = hiz_5_1;

#line 737
    (&kernelContext_11)->scene_color_0 = scene_color_1;

    thread uint width_1;
    thread uint height_1;



    (*((&width_1)) = (scene_depth_1).get_width(0)),(*((&height_1)) = (scene_depth_1).get_height(0));
    int _S63 = int(width_1);

#line 745
    int _S64 = int(height_1);

#line 745
    int2 extent_7 = int2(_S63, _S64);
    float _S65 = float(width_1);

#line 746
    float _S66 = float(height_1);

#line 746
    float2 size_4 = float2(_S65, _S66);
    int2 _S67 = int2(position_0.xy);

#line 754
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S68 = int3(_S67, int(0));

#line 756
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S68)).xy), uint(((_S68)).z)));
    float _S69 = surface_0.w;

#line 757
    float sharpness_0 = sharpness_of_0(_S69);

#line 757
    float _S70 = depth_at_0(_S67, extent_7, &kernelContext_11);


    if(_S70 <= 0.0f)
    {

#line 760
        pixelOutput_0 _S71 = { NOTHING_0 };

        return _S71;
    }

#line 762
    float3 _S72 = view_position_0(_S67, _S70, size_4, &kernelContext_11);

#line 762
    float3 _S73 = normal_at_0(_S67, _S72, extent_7, size_4, &kernelContext_11);

#line 768
    float3 towards_0 = normalize(_S72);
    float3 ray_0 = reflect(towards_0, _S73);


    float4 _S74 = float4(ray_0, 0.0f);

#line 772
    float3 reflection_direction_0 = normalize((((_S74) * (matrix<float,int(4),int(4)> ((&kernelContext_11)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 772
    float3 _S75 = probe_environment_0((((float4(_S72, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_11)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_11)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, reflection_direction_0, &kernelContext_11);

#line 772
    float3 _S76 = sky_prefiltered_0(reflection_direction_0, _S69, &kernelContext_11);

#line 786
    float3 environment_0 = _S75 + _S76;

#line 794
    float3 _S77 = - towards_0;
    float3 f0_0 = surface_0.xyz;

#line 795
    float2 _S78 = dfg_at_0(saturate(dot(_S73, _S77)), _S69, &kernelContext_11);

    float3 env_brdf_0 = f0_0 * float3(_S78.x)  + float3(_S78.y) ;

#line 802
    if(sharpness_0 <= 0.0f)
    {

#line 802
        pixelOutput_0 _S79 = { float4(environment_0 * env_brdf_0, 0.0f) };

        return _S79;
    }


    float _S80 = saturate((1.0f - dot(ray_0, _S77)) / 0.05000000074505806f);


    float _S81 = _S72.z;

#line 811
    float3 start_0 = _S72 + _S73 * float3((abs(_S81) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_11)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_11)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_11)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_11)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_11)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_11)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_11)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_11)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_11)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_11)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_11)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_11)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_11)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_11)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_11)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_11)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S74) * (matrix<float,int(4),int(4)> ((&kernelContext_11)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_11)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_11)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_11)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_11)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_11)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_11)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_11)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_11)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_11)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_11)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_11)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_11)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_11)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_11)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_11)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S82 = clip_start_0.w;

#line 816
    if(_S82 <= 0.0f)
    {

#line 816
        pixelOutput_0 _S83 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S83;
    }
    float2 _S84 = clip_start_0.xy;

#line 820
    float2 _S85 = float2(_S82) ;

#line 820
    float2 at_start_0 = pixel_of_0(_S84 / _S85, size_4);

#line 826
    float2 _S86 = clip_ray_0.xy;

#line 826
    float _S87 = clip_ray_0.w;

#line 826
    float2 _S88 = float2(_S87) ;

#line 826
    float2 ndc_rate_0 = (_S86 * _S85 - _S84 * _S88) / float2((_S82 * _S82)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S65, - ndc_rate_0.y * 0.5f * _S66);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 829
        pixelOutput_0 _S89 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S89;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 840
    float reach_1 = 0.75f * min(_S65, _S66);

    float _S90 = forward_1.x;

#line 842
    float travel_0;

#line 842
    if(_S90 > 0.0f)
    {

#line 842
        travel_0 = min(reach_1, (_S65 - 1.0f - at_start_0.x) / _S90);

#line 842
    }
    else
    {

        if(_S90 < 0.0f)
        {

#line 846
            travel_0 = min(reach_1, - at_start_0.x / _S90);

#line 846
        }
        else
        {

#line 846
            travel_0 = reach_1;

#line 846
        }

#line 842
    }

#line 850
    float _S91 = forward_1.y;

#line 850
    if(_S91 > 0.0f)
    {

#line 850
        travel_0 = min(travel_0, (_S66 - 1.0f - at_start_0.y) / _S91);

#line 850
    }
    else
    {

        if(_S91 < 0.0f)
        {

#line 854
            travel_0 = min(travel_0, - at_start_0.y / _S91);

#line 854
        }

#line 850
    }

#line 862
    if(_S87 > 0.0f)
    {

#line 862
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S86 / _S88, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 862
    }
    else
    {

#line 877
        if(_S87 < 0.0f)
        {

#line 884
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_11)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_11)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 889
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S82) / _S87)) ;

#line 889
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 877
        }

#line 862
    }

#line 896
    float _S92 = max(travel_0, 0.0f);
    if(_S92 <= 0.00390625f)
    {

#line 897
        pixelOutput_0 _S93 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S93;
    }

#line 906
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S92) , size_4);

#line 906
    float when_end_0;

    if((abs(_S90)) >= (abs(_S91)))
    {

#line 908
        float _S94 = ndc_end_0.x;

#line 908
        when_end_0 = (_S94 * _S82 - clip_start_0.x) / (clip_ray_0.x - _S94 * _S87);

#line 908
    }
    else
    {

#line 909
        float _S95 = ndc_end_0.y;

#line 909
        when_end_0 = (_S95 * _S82 - clip_start_0.y) / (clip_ray_0.y - _S95 * _S87);

#line 908
    }

#line 908
    bool _S96;

#line 916
    if(!(when_end_0 > 0.0f))
    {

#line 916
        _S96 = true;

#line 916
    }
    else
    {

#line 916
        _S96 = !isfinite(when_end_0);

#line 916
    }

#line 916
    if(_S96)
    {

#line 916
        pixelOutput_0 _S97 = { float4(environment_0 * env_brdf_0, sharpness_0) };

        return _S97;
    }

#line 924
    float inverse_w_start_0 = 1.0f / _S82;

    float inverse_w_end_0 = 1.0f / (_S82 + when_end_0 * _S87);
    float _S98 = start_0.z;

#line 927
    float _S99 = _S98 * inverse_w_start_0;
    float _S100 = (_S98 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 933
    float3 _S101 = environment_0 * env_brdf_0;
    uint _S102 = min((&kernelContext_11)->camera_0->hiz_0.x, 5U);

#line 964
    float _S103 = _S98 - _S81;

#line 964
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S92), _S92);

#line 964
    float previous_gap_0 = _S103;

#line 964
    float entry_z_0 = _S98;

#line 964
    uint step_0 = 0U;

#line 964
    uint level_1 = 0U;

    for(;;)
    {

#line 966
        if(step_0 < 96U)
        {
        }
        else
        {

#line 966
            reflection_0 = _S101;

#line 966
            break;
        }
        float cell_1 = float(1U << level_1);
        float2 at_4 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S104 = min(at_travel_0 + cell_exit_0(at_4, forward_1, cell_1, _S92), _S92);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S104) ;
        float along_0 = _S104 / _S92;

        float exit_z_0 = mix(_S99, _S100, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 974
        float _S105 = hiz_at_0(level_1, int2(floor(at_4 / float2(cell_1) )), int2(_S63 >> level_1, _S64 >> level_1), &kernelContext_11);

#line 974
        float gap_0;

#line 983
        if(_S105 <= 0.0f)
        {

#line 983
            gap_0 = 1.0f;

#line 983
        }
        else
        {

#line 983
            float _S106 = view_z_of_0(_S105, &kernelContext_11);

#line 983
            gap_0 = exit_z_0 - _S106;

#line 983
        }

#line 992
        bool _S107 = !(gap_0 > 0.0f);

#line 992
        if(_S107)
        {

#line 992
            _S96 = level_1 > 0U;

#line 992
        }
        else
        {

#line 992
            _S96 = false;

#line 992
        }

#line 992
        if(_S96)
        {

#line 992
            level_1 = level_1 - 1U;

#line 998
            step_0 = step_0 + 1U;

#line 966
            continue;
        }

#line 966
        bool _S108;

#line 1001
        if(_S107)
        {

#line 1001
            _S108 = previous_gap_0 > 0.0f;

#line 1001
        }
        else
        {

#line 1001
            _S108 = false;

#line 1001
        }

#line 1001
        if(_S108)
        {



            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {

#line 1014
                float2 hit_at_0 = mix(at_4, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1029
                float confidence_0 = sharpness_0 * _S80 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S104 / reach_1) / 0.25f) * saturate(1.0f - behind_0 / thickness_0);
                int3 _S109 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_7 - int2(int(1), int(1))), int(0));

#line 1030
                reflection_0 = (((&kernelContext_11)->scene_color_0).read(vec<uint,2>(((_S109)).xy), uint(((_S109)).z))).xyz * env_brdf_0 * float3(confidence_0)  + _S101 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 1001
        }

#line 1042
        if(_S104 >= _S92)
        {

#line 1042
            reflection_0 = _S101;

            break;
        }



        uint _S110 = min(level_1 + 1U, _S102);

#line 1049
        at_travel_0 = _S104;

#line 1049
        previous_gap_0 = gap_0;

#line 1049
        entry_z_0 = exit_z_0;

#line 1049
        level_1 = _S110;

#line 966
        step_0 = step_0 + 1U;

#line 966
    }

#line 966
    pixelOutput_0 _S111 = { float4(reflection_0, sharpness_0) };

#line 1057
    return _S111;
}


#line 1057
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
    thread KernelContext_0 kernelContext_12;

#line 417
    (&kernelContext_12)->scene_depth_0 = scene_depth_2;

#line 417
    (&kernelContext_12)->reflectivity_0 = reflectivity_2;

#line 417
    (&kernelContext_12)->camera_0 = camera_2;

#line 417
    (&kernelContext_12)->probes_0 = probes_2;

#line 417
    (&kernelContext_12)->sky_prefilter_0 = sky_prefilter_2;

#line 417
    (&kernelContext_12)->dfg_0 = dfg_2;

#line 417
    (&kernelContext_12)->hiz_1_0 = hiz_1_2;

#line 417
    (&kernelContext_12)->hiz_2_0 = hiz_2_2;

#line 417
    (&kernelContext_12)->hiz_3_0 = hiz_3_2;

#line 417
    (&kernelContext_12)->hiz_4_0 = hiz_4_2;

#line 417
    (&kernelContext_12)->hiz_5_0 = hiz_5_2;

#line 417
    (&kernelContext_12)->scene_color_0 = scene_color_2;

#line 728
    thread FullscreenOutput_0 output_1;


    float2 _S112 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 731
    (&output_1)->uv_2 = _S112;
    (&output_1)->position_2 = float4(_S112 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 732
    thread vertexMain_Result_0 _S113;

#line 732
    (&_S113)->position_1 = output_1.position_2;

#line 732
    (&_S113)->uv_1 = output_1.uv_2;

#line 732
    return _S113;
}

