#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 272 "shaders/ssr.slang"
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
    depth2d<float, access::sample> hiz_1_0;
    depth2d<float, access::sample> hiz_2_0;
    depth2d<float, access::sample> hiz_3_0;
    depth2d<float, access::sample> hiz_4_0;
    depth2d<float, access::sample> hiz_5_0;
    texture2d<float, access::sample> scene_color_0;
};


#line 418 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 421
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 418
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 421
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 430
float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 440
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 430
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 440
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 456
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 458
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 458
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 459
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 459
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 460
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 460
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 461
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 461
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 463
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 464
        horizontal_0 = _S8 - centre_0;

#line 464
    }
    else
    {

#line 464
        horizontal_0 = centre_0 - _S5;

#line 464
    }

#line 464
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 467
        vertical_0 = _S14 - centre_0;

#line 467
    }
    else
    {

#line 467
        vertical_0 = centre_0 - _S11;

#line 467
    }

#line 477
    return normalize(cross(vertical_0, horizontal_0));
}


#line 139
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 656
float3 probe_environment_0(float3 world_position_0, float3 direction_0, KernelContext_0 thread* kernelContext_5)
{

#line 656
    float3 _S16 = float3(1.0f) ;

    float3 _S17 = float3(0.0f, 0.0f, 0.0f);

#line 658
    float3 last_0 = max(float3(kernelContext_5->camera_0->probe_counts_0.xyz) - _S16, _S17);
    float3 grid_0 = clamp((world_position_0 - kernelContext_5->camera_0->probe_origin_0.xyz) * kernelContext_5->camera_0->probe_inv_spacing_0.xyz, _S17, last_0);

    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S18 = uint3(base_0);
    uint3 _S19 = uint3(min(base_0 + _S16, last_0));
    uint total_0 = max(kernelContext_5->camera_0->probe_counts_0.w, 1U) - 1U;
    uint _S20 = _S18.z;

#line 666
    uint _S21 = _S18.y;

#line 666
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

#line 683
    float4 _S27 = float4(f_0.y) ;

#line 683
    float4 _S28 = mix(mix(float4(x00_0.sh_r_0) , float4(y00_0.sh_r_0) , _S26), mix(float4(x10_0.sh_r_0) , float4(y10_0.sh_r_0) , _S26), _S27);

#line 683
    (&z0_0)->sh_r_0 = _S28;
    float4 _S29 = mix(mix(float4(x00_0.sh_g_0) , float4(y00_0.sh_g_0) , _S26), mix(float4(x10_0.sh_g_0) , float4(y10_0.sh_g_0) , _S26), _S27);

#line 684
    (&z0_0)->sh_g_0 = _S29;
    float4 _S30 = mix(mix(float4(x00_0.sh_b_0) , float4(y00_0.sh_b_0) , _S26), mix(float4(x10_0.sh_b_0) , float4(y10_0.sh_b_0) , _S26), _S27);

#line 685
    (&z0_0)->sh_b_0 = _S30;
    thread GpuProbe_0 z1_0;
    float4 _S31 = mix(mix(float4(x01_0.sh_r_0) , float4(y01_0.sh_r_0) , _S26), mix(float4(x11_0.sh_r_0) , float4(y11_0.sh_r_0) , _S26), _S27);

#line 687
    (&z1_0)->sh_r_0 = _S31;
    float4 _S32 = mix(mix(float4(x01_0.sh_g_0) , float4(y01_0.sh_g_0) , _S26), mix(float4(x11_0.sh_g_0) , float4(y11_0.sh_g_0) , _S26), _S27);

#line 688
    (&z1_0)->sh_g_0 = _S32;
    float4 _S33 = mix(mix(float4(x01_0.sh_b_0) , float4(y01_0.sh_b_0) , _S26), mix(float4(x11_0.sh_b_0) , float4(y11_0.sh_b_0) , _S26), _S27);

#line 689
    (&z1_0)->sh_b_0 = _S33;
    thread GpuProbe_0 cell_0;
    float4 _S34 = float4(f_0.z) ;

#line 691
    float4 _S35 = mix(_S28, _S31, _S34);

#line 691
    (&cell_0)->sh_r_0 = _S35;
    float4 _S36 = mix(_S29, _S32, _S34);

#line 692
    (&cell_0)->sh_g_0 = _S36;
    float4 _S37 = mix(_S30, _S33, _S34);

#line 693
    (&cell_0)->sh_b_0 = _S37;

#line 693
    float3 _S38 = float3(2.09439516067504883f) ;
    return max(float3(dot(_S35.xyz / _S38, direction_0) + _S35.w / 3.14159274101257324f, dot(_S36.xyz / _S38, direction_0) + _S36.w / 3.14159274101257324f, dot(_S37.xyz / _S38, direction_0) + _S37.w / 3.14159274101257324f), _S17);
}


#line 600
float2 decode_sky_weights_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 612
float2 sky_prefilter_at_0(float up_0, float roughness_1, KernelContext_0 thread* kernelContext_6)
{

#line 612
    texture2d<float, access::sample> _S39 = kernelContext_6->sky_prefilter_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S39).get_width(0)),(*((&height_0)) = (_S39).get_height(0));
    float2 extent_5 = float2(float(width_0), float(height_0));
    float2 scaled_0 = float2(saturate(up_0), saturate(roughness_1)) * extent_5 - float2(0.5f) ;

#line 618
    float2 _S40 = float2(1.0f) ;
    float2 _S41 = extent_5 - _S40;

#line 619
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S41);

    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );

    int2 _S42 = int2(low_0);
    int2 _S43 = int2(min(low_0 + _S40, _S41));
    int _S44 = _S42.x;

#line 625
    int _S45 = _S42.y;

#line 625
    int3 _S46 = int3(_S44, _S45, int(0));
    int _S47 = _S43.x;

#line 626
    int3 _S48 = int3(_S47, _S45, int(0));
    float2 _S49 = float2(weight_0.x) ;
    int _S50 = _S43.y;

#line 628
    int3 _S51 = int3(_S44, _S50, int(0));
    int3 _S52 = int3(_S47, _S50, int(0));

    return mix(mix(decode_sky_weights_0(((kernelContext_6->sky_prefilter_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z)))), decode_sky_weights_0(((kernelContext_6->sky_prefilter_0).read(vec<uint,2>(((_S48)).xy), uint(((_S48)).z)))), _S49), mix(decode_sky_weights_0(((kernelContext_6->sky_prefilter_0).read(vec<uint,2>(((_S51)).xy), uint(((_S51)).z)))), decode_sky_weights_0(((kernelContext_6->sky_prefilter_0).read(vec<uint,2>(((_S52)).xy), uint(((_S52)).z)))), _S49), float2(weight_0.y) );
}


#line 646
float3 sky_prefiltered_0(float3 direction_1, float roughness_2, KernelContext_0 thread* kernelContext_7)
{
    float up_1 = clamp(direction_1.y, -1.0f, 1.0f);

#line 648
    float2 _S53 = sky_prefilter_at_0(abs(up_1), roughness_2, kernelContext_7);

    bool _S54 = up_1 >= 0.0f;

#line 650
    float3 far_0;

#line 650
    if(_S54)
    {

#line 650
        far_0 = kernelContext_7->camera_0->sky_0[int(0)].xyz;

#line 650
    }
    else
    {

#line 650
        far_0 = kernelContext_7->camera_0->sky_0[int(2)].xyz;

#line 650
    }

#line 650
    float3 opposite_0;
    if(_S54)
    {

#line 651
        opposite_0 = kernelContext_7->camera_0->sky_0[int(2)].xyz;

#line 651
    }
    else
    {

#line 651
        opposite_0 = kernelContext_7->camera_0->sky_0[int(0)].xyz;

#line 651
    }
    float _S55 = _S53.x;

#line 652
    float _S56 = _S53.y;
    return kernelContext_7->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S55 - _S56))  + far_0 * float3(_S55)  + opposite_0 * float3(_S56) ;
}


#line 486
float2 pixel_of_0(float2 ndc_0, float2 size_1)
{
    return float2((ndc_0.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_0.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_0, float2 size_2)
{
    return float2(at_0.x / size_2.x * 2.0f - 1.0f, 1.0f - at_0.y / size_2.y * 2.0f);
}


#line 563
float cell_exit_0(float2 at_1, float2 forward_0, float size_3, float reach_0)
{

    float _S57 = forward_0.x;

#line 566
    bool _S58 = _S57 > 0.0f;

#line 566
    float along_x_0;

#line 566
    if(_S58)
    {

#line 566
        along_x_0 = (floor(at_1.x / size_3) + 1.0f) * size_3;

#line 566
    }
    else
    {

#line 566
        along_x_0 = floor(at_1.x / size_3) * size_3;

#line 566
    }
    float _S59 = forward_0.y;

#line 567
    bool _S60 = _S59 > 0.0f;

#line 567
    float along_y_0;

#line 567
    if(_S60)
    {

#line 567
        along_y_0 = (floor(at_1.y / size_3) + 1.0f) * size_3;

#line 567
    }
    else
    {

#line 567
        along_y_0 = floor(at_1.y / size_3) * size_3;

#line 567
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 568
    float _S61;

    if((abs(_S57)) < 9.99999997475242708e-07f)
    {

#line 570
        along_x_0 = reach_0;

#line 570
    }
    else
    {

#line 571
        if(_S58)
        {

#line 571
            _S61 = nudge_0;

#line 571
        }
        else
        {

#line 571
            _S61 = - nudge_0;

#line 571
        }

#line 571
        along_x_0 = (along_x_0 + _S61 - at_1.x) / _S57;

#line 570
    }


    if((abs(_S59)) < 9.99999997475242708e-07f)
    {

#line 573
        along_y_0 = reach_0;

#line 573
    }
    else
    {

#line 574
        if(_S60)
        {

#line 574
            _S61 = nudge_0;

#line 574
        }
        else
        {

#line 574
            _S61 = - nudge_0;

#line 574
        }

#line 574
        along_y_0 = (along_y_0 + _S61 - at_1.y) / _S59;

#line 573
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 522
float hiz_at_0(uint level_0, int2 texel_1, int2 extent_6, KernelContext_0 thread* kernelContext_8)
{
    int2 _S62 = int2(int(0), int(0));
    int3 at_2 = int3(clamp(texel_1, _S62, max(extent_6 - int2(int(1), int(1)), _S62)), int(0));
    switch(level_0)
    {
    case 0U:
        {

#line 529
            return ((kernelContext_8->scene_depth_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 1U:
        {

#line 531
            return ((kernelContext_8->hiz_1_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 2U:
        {

#line 533
            return ((kernelContext_8->hiz_2_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 3U:
        {

#line 535
            return ((kernelContext_8->hiz_3_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 4U:
        {

#line 537
            return ((kernelContext_8->hiz_4_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    default:
        {

#line 539
            return ((kernelContext_8->hiz_5_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    }

#line 539
}


#line 550
float view_z_of_0(float depth_2, KernelContext_0 thread* kernelContext_9)
{
    float4 view_2 = (((float4(0.0f, 0.0f, depth_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_9->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_9->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_9->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_9->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_9->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_9->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_9->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_9->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_9->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_9->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_9->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_9->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_9->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_9->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_9->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_9->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_2.z / view_2.w;
}


#line 505
float thickness_at_0(float advance_0, float depth_3)
{
    return max(advance_0, abs(depth_3) * 0.01999999955296516f);
}


#line 507
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 507
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 713
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S63 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(8)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 713
    float3 reflection_0;

#line 713
    thread KernelContext_0 kernelContext_10;

#line 713
    (&kernelContext_10)->scene_depth_0 = scene_depth_1;

#line 713
    (&kernelContext_10)->reflectivity_0 = reflectivity_1;

#line 713
    (&kernelContext_10)->camera_0 = camera_1;

#line 713
    (&kernelContext_10)->probes_0 = probes_1;

#line 713
    (&kernelContext_10)->sky_prefilter_0 = sky_prefilter_1;

#line 713
    (&kernelContext_10)->hiz_1_0 = hiz_1_1;

#line 713
    (&kernelContext_10)->hiz_2_0 = hiz_2_1;

#line 713
    (&kernelContext_10)->hiz_3_0 = hiz_3_1;

#line 713
    (&kernelContext_10)->hiz_4_0 = hiz_4_1;

#line 713
    (&kernelContext_10)->hiz_5_0 = hiz_5_1;

#line 713
    (&kernelContext_10)->scene_color_0 = scene_color_1;

    thread uint width_1;
    thread uint height_1;



    (*((&width_1)) = (scene_depth_1).get_width(0)),(*((&height_1)) = (scene_depth_1).get_height(0));
    int _S64 = int(width_1);

#line 721
    int _S65 = int(height_1);

#line 721
    int2 extent_7 = int2(_S64, _S65);
    float _S66 = float(width_1);

#line 722
    float _S67 = float(height_1);

#line 722
    float2 size_4 = float2(_S66, _S67);
    int2 _S68 = int2(position_0.xy);

#line 730
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S69 = int3(_S68, int(0));

#line 732
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S69)).xy), uint(((_S69)).z)));
    float _S70 = surface_0.w;

#line 733
    float sharpness_0 = sharpness_of_0(_S70);

#line 733
    float _S71 = depth_at_0(_S68, extent_7, &kernelContext_10);


    if(_S71 <= 0.0f)
    {

#line 736
        pixelOutput_0 _S72 = { NOTHING_0 };

        return _S72;
    }

#line 738
    float3 _S73 = view_position_0(_S68, _S71, size_4, &kernelContext_10);

#line 738
    float3 _S74 = normal_at_0(_S68, _S73, extent_7, size_4, &kernelContext_10);

#line 744
    float3 towards_0 = normalize(_S73);
    float3 ray_0 = reflect(towards_0, _S74);


    float4 _S75 = float4(ray_0, 0.0f);

#line 748
    float3 reflection_direction_0 = normalize((((_S75) * (matrix<float,int(4),int(4)> ((&kernelContext_10)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 748
    float3 _S76 = probe_environment_0((((float4(_S73, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_10)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_10)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, reflection_direction_0, &kernelContext_10);

#line 748
    float3 _S77 = sky_prefiltered_0(reflection_direction_0, _S70, &kernelContext_10);

#line 762
    float3 environment_0 = _S76 + _S77;

#line 767
    float3 _S78 = - towards_0;
    float3 f0_0 = surface_0.xyz;
    float grazing_0 = 1.0f - saturate(dot(_S74, _S78));
    float grazing2_0 = grazing_0 * grazing_0;
    float3 fresnel_0 = f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) ;

#line 776
    if(sharpness_0 <= 0.0f)
    {

#line 776
        pixelOutput_0 _S79 = { float4(environment_0 * fresnel_0, 0.0f) };

        return _S79;
    }


    float _S80 = saturate((1.0f - dot(ray_0, _S78)) / 0.05000000074505806f);


    float _S81 = _S73.z;

#line 785
    float3 start_0 = _S73 + _S74 * float3((abs(_S81) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_10)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_10)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_10)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_10)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_10)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_10)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_10)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_10)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_10)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_10)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_10)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_10)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_10)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_10)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_10)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_10)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S75) * (matrix<float,int(4),int(4)> ((&kernelContext_10)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_10)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_10)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_10)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_10)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_10)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_10)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_10)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_10)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_10)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_10)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_10)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_10)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_10)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_10)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_10)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S82 = clip_start_0.w;

#line 790
    if(_S82 <= 0.0f)
    {

#line 790
        pixelOutput_0 _S83 = { float4(environment_0 * fresnel_0, sharpness_0) };

        return _S83;
    }
    float2 _S84 = clip_start_0.xy;

#line 794
    float2 _S85 = float2(_S82) ;

#line 794
    float2 at_start_0 = pixel_of_0(_S84 / _S85, size_4);

#line 800
    float2 _S86 = clip_ray_0.xy;

#line 800
    float _S87 = clip_ray_0.w;

#line 800
    float2 _S88 = float2(_S87) ;

#line 800
    float2 ndc_rate_0 = (_S86 * _S85 - _S84 * _S88) / float2((_S82 * _S82)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S66, - ndc_rate_0.y * 0.5f * _S67);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 803
        pixelOutput_0 _S89 = { float4(environment_0 * fresnel_0, sharpness_0) };

        return _S89;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 814
    float reach_1 = 0.75f * min(_S66, _S67);

    float _S90 = forward_1.x;

#line 816
    float travel_0;

#line 816
    if(_S90 > 0.0f)
    {

#line 816
        travel_0 = min(reach_1, (_S66 - 1.0f - at_start_0.x) / _S90);

#line 816
    }
    else
    {

        if(_S90 < 0.0f)
        {

#line 820
            travel_0 = min(reach_1, - at_start_0.x / _S90);

#line 820
        }
        else
        {

#line 820
            travel_0 = reach_1;

#line 820
        }

#line 816
    }

#line 824
    float _S91 = forward_1.y;

#line 824
    if(_S91 > 0.0f)
    {

#line 824
        travel_0 = min(travel_0, (_S67 - 1.0f - at_start_0.y) / _S91);

#line 824
    }
    else
    {

        if(_S91 < 0.0f)
        {

#line 828
            travel_0 = min(travel_0, - at_start_0.y / _S91);

#line 828
        }

#line 824
    }

#line 836
    if(_S87 > 0.0f)
    {

#line 836
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S86 / _S88, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 836
    }
    else
    {

#line 851
        if(_S87 < 0.0f)
        {

#line 858
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_10)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_10)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 863
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S82) / _S87)) ;

#line 863
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 851
        }

#line 836
    }

#line 870
    float _S92 = max(travel_0, 0.0f);
    if(_S92 <= 0.00390625f)
    {

#line 871
        pixelOutput_0 _S93 = { float4(environment_0 * fresnel_0, sharpness_0) };

        return _S93;
    }

#line 880
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S92) , size_4);

#line 880
    float when_end_0;

    if((abs(_S90)) >= (abs(_S91)))
    {

#line 882
        float _S94 = ndc_end_0.x;

#line 882
        when_end_0 = (_S94 * _S82 - clip_start_0.x) / (clip_ray_0.x - _S94 * _S87);

#line 882
    }
    else
    {

#line 883
        float _S95 = ndc_end_0.y;

#line 883
        when_end_0 = (_S95 * _S82 - clip_start_0.y) / (clip_ray_0.y - _S95 * _S87);

#line 882
    }

#line 882
    bool _S96;

#line 890
    if(!(when_end_0 > 0.0f))
    {

#line 890
        _S96 = true;

#line 890
    }
    else
    {

#line 890
        _S96 = !isfinite(when_end_0);

#line 890
    }

#line 890
    if(_S96)
    {

#line 890
        pixelOutput_0 _S97 = { float4(environment_0 * fresnel_0, sharpness_0) };

        return _S97;
    }

#line 898
    float inverse_w_start_0 = 1.0f / _S82;

    float inverse_w_end_0 = 1.0f / (_S82 + when_end_0 * _S87);
    float _S98 = start_0.z;

#line 901
    float _S99 = _S98 * inverse_w_start_0;
    float _S100 = (_S98 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 907
    float3 _S101 = environment_0 * fresnel_0;
    uint _S102 = min((&kernelContext_10)->camera_0->hiz_0.x, 5U);

#line 938
    float _S103 = _S98 - _S81;

#line 938
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S92), _S92);

#line 938
    float previous_gap_0 = _S103;

#line 938
    float entry_z_0 = _S98;

#line 938
    uint step_0 = 0U;

#line 938
    uint level_1 = 0U;

    for(;;)
    {

#line 940
        if(step_0 < 96U)
        {
        }
        else
        {

#line 940
            reflection_0 = _S101;

#line 940
            break;
        }
        float cell_1 = float(1U << level_1);
        float2 at_3 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S104 = min(at_travel_0 + cell_exit_0(at_3, forward_1, cell_1, _S92), _S92);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S104) ;
        float along_0 = _S104 / _S92;

        float exit_z_0 = mix(_S99, _S100, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 948
        float _S105 = hiz_at_0(level_1, int2(floor(at_3 / float2(cell_1) )), int2(_S64 >> level_1, _S65 >> level_1), &kernelContext_10);

#line 948
        float gap_0;

#line 957
        if(_S105 <= 0.0f)
        {

#line 957
            gap_0 = 1.0f;

#line 957
        }
        else
        {

#line 957
            float _S106 = view_z_of_0(_S105, &kernelContext_10);

#line 957
            gap_0 = exit_z_0 - _S106;

#line 957
        }

#line 966
        bool _S107 = !(gap_0 > 0.0f);

#line 966
        if(_S107)
        {

#line 966
            _S96 = level_1 > 0U;

#line 966
        }
        else
        {

#line 966
            _S96 = false;

#line 966
        }

#line 966
        if(_S96)
        {

#line 966
            level_1 = level_1 - 1U;

#line 972
            step_0 = step_0 + 1U;

#line 940
            continue;
        }

#line 940
        bool _S108;

#line 975
        if(_S107)
        {

#line 975
            _S108 = previous_gap_0 > 0.0f;

#line 975
        }
        else
        {

#line 975
            _S108 = false;

#line 975
        }

#line 975
        if(_S108)
        {



            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {

#line 988
                float2 hit_at_0 = mix(at_3, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 1003
                float confidence_0 = sharpness_0 * _S80 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S104 / reach_1) / 0.25f) * saturate(1.0f - behind_0 / thickness_0);
                int3 _S109 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_7 - int2(int(1), int(1))), int(0));

#line 1004
                reflection_0 = (((&kernelContext_10)->scene_color_0).read(vec<uint,2>(((_S109)).xy), uint(((_S109)).z))).xyz * fresnel_0 * float3(confidence_0)  + _S101 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 975
        }

#line 1016
        if(_S104 >= _S92)
        {

#line 1016
            reflection_0 = _S101;

            break;
        }



        uint _S110 = min(level_1 + 1U, _S102);

#line 1023
        at_travel_0 = _S104;

#line 1023
        previous_gap_0 = gap_0;

#line 1023
        entry_z_0 = exit_z_0;

#line 1023
        level_1 = _S110;

#line 940
        step_0 = step_0 + 1U;

#line 940
    }

#line 940
    pixelOutput_0 _S111 = { float4(reflection_0, sharpness_0) };

#line 1031
    return _S111;
}


#line 1031
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 406
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 406
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(8)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 406
    thread KernelContext_0 kernelContext_11;

#line 406
    (&kernelContext_11)->scene_depth_0 = scene_depth_2;

#line 406
    (&kernelContext_11)->reflectivity_0 = reflectivity_2;

#line 406
    (&kernelContext_11)->camera_0 = camera_2;

#line 406
    (&kernelContext_11)->probes_0 = probes_2;

#line 406
    (&kernelContext_11)->sky_prefilter_0 = sky_prefilter_2;

#line 406
    (&kernelContext_11)->hiz_1_0 = hiz_1_2;

#line 406
    (&kernelContext_11)->hiz_2_0 = hiz_2_2;

#line 406
    (&kernelContext_11)->hiz_3_0 = hiz_3_2;

#line 406
    (&kernelContext_11)->hiz_4_0 = hiz_4_2;

#line 406
    (&kernelContext_11)->hiz_5_0 = hiz_5_2;

#line 406
    (&kernelContext_11)->scene_color_0 = scene_color_2;

#line 704
    thread FullscreenOutput_0 output_1;


    float2 _S112 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 707
    (&output_1)->uv_2 = _S112;
    (&output_1)->position_2 = float4(_S112 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 708
    thread vertexMain_Result_0 _S113;

#line 708
    (&_S113)->position_1 = output_1.position_2;

#line 708
    (&_S113)->uv_1 = output_1.uv_2;

#line 708
    return _S113;
}

