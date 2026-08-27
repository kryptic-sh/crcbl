#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

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
    depth2d<float, access::sample> hiz_1_0;
    depth2d<float, access::sample> hiz_2_0;
    depth2d<float, access::sample> hiz_3_0;
    depth2d<float, access::sample> hiz_4_0;
    depth2d<float, access::sample> hiz_5_0;
    texture2d<float, access::sample> scene_color_0;
};


#line 375 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 378
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 375
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 378
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 387
float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 397
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 387
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 397
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 413
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 415
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 415
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 416
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 416
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 417
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 417
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 418
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 418
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 420
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 421
        horizontal_0 = _S8 - centre_0;

#line 421
    }
    else
    {

#line 421
        horizontal_0 = centre_0 - _S5;

#line 421
    }

#line 421
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 424
        vertical_0 = _S14 - centre_0;

#line 424
    }
    else
    {

#line 424
        vertical_0 = centre_0 - _S11;

#line 424
    }

#line 434
    return normalize(cross(vertical_0, horizontal_0));
}


#line 122
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 547
float3 probe_environment_0(float3 world_position_0, float3 direction_0, KernelContext_0 thread* kernelContext_5)
{

#line 547
    float3 _S16 = float3(1.0f) ;

    float3 _S17 = float3(0.0f, 0.0f, 0.0f);

#line 549
    float3 last_0 = max(float3(kernelContext_5->camera_0->probe_counts_0.xyz) - _S16, _S17);
    float3 grid_0 = clamp((world_position_0 - kernelContext_5->camera_0->probe_origin_0.xyz) * kernelContext_5->camera_0->probe_inv_spacing_0.xyz, _S17, last_0);

    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S18 = uint3(base_0);
    uint3 _S19 = uint3(min(base_0 + _S16, last_0));
    uint total_0 = max(kernelContext_5->camera_0->probe_counts_0.w, 1U) - 1U;
    uint _S20 = _S18.z;

#line 557
    uint _S21 = _S18.y;

#line 557
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

#line 574
    float4 _S27 = float4(f_0.y) ;

#line 574
    float4 _S28 = mix(mix(float4(x00_0.sh_r_0) , float4(y00_0.sh_r_0) , _S26), mix(float4(x10_0.sh_r_0) , float4(y10_0.sh_r_0) , _S26), _S27);

#line 574
    (&z0_0)->sh_r_0 = _S28;
    float4 _S29 = mix(mix(float4(x00_0.sh_g_0) , float4(y00_0.sh_g_0) , _S26), mix(float4(x10_0.sh_g_0) , float4(y10_0.sh_g_0) , _S26), _S27);

#line 575
    (&z0_0)->sh_g_0 = _S29;
    float4 _S30 = mix(mix(float4(x00_0.sh_b_0) , float4(y00_0.sh_b_0) , _S26), mix(float4(x10_0.sh_b_0) , float4(y10_0.sh_b_0) , _S26), _S27);

#line 576
    (&z0_0)->sh_b_0 = _S30;
    thread GpuProbe_0 z1_0;
    float4 _S31 = mix(mix(float4(x01_0.sh_r_0) , float4(y01_0.sh_r_0) , _S26), mix(float4(x11_0.sh_r_0) , float4(y11_0.sh_r_0) , _S26), _S27);

#line 578
    (&z1_0)->sh_r_0 = _S31;
    float4 _S32 = mix(mix(float4(x01_0.sh_g_0) , float4(y01_0.sh_g_0) , _S26), mix(float4(x11_0.sh_g_0) , float4(y11_0.sh_g_0) , _S26), _S27);

#line 579
    (&z1_0)->sh_g_0 = _S32;
    float4 _S33 = mix(mix(float4(x01_0.sh_b_0) , float4(y01_0.sh_b_0) , _S26), mix(float4(x11_0.sh_b_0) , float4(y11_0.sh_b_0) , _S26), _S27);

#line 580
    (&z1_0)->sh_b_0 = _S33;
    thread GpuProbe_0 cell_0;
    float4 _S34 = float4(f_0.z) ;

#line 582
    float4 _S35 = mix(_S28, _S31, _S34);

#line 582
    (&cell_0)->sh_r_0 = _S35;
    float4 _S36 = mix(_S29, _S32, _S34);

#line 583
    (&cell_0)->sh_g_0 = _S36;
    float4 _S37 = mix(_S30, _S33, _S34);

#line 584
    (&cell_0)->sh_b_0 = _S37;

#line 584
    float3 _S38 = float3(2.09439516067504883f) ;
    return max(float3(dot(_S35.xyz / _S38, direction_0) + _S35.w / 3.14159274101257324f, dot(_S36.xyz / _S38, direction_0) + _S36.w / 3.14159274101257324f, dot(_S37.xyz / _S38, direction_0) + _S37.w / 3.14159274101257324f), _S17);
}


#line 443
float2 pixel_of_0(float2 ndc_0, float2 size_1)
{
    return float2((ndc_0.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_0.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_0, float2 size_2)
{
    return float2(at_0.x / size_2.x * 2.0f - 1.0f, 1.0f - at_0.y / size_2.y * 2.0f);
}


#line 520
float cell_exit_0(float2 at_1, float2 forward_0, float size_3, float reach_0)
{

    float _S39 = forward_0.x;

#line 523
    bool _S40 = _S39 > 0.0f;

#line 523
    float along_x_0;

#line 523
    if(_S40)
    {

#line 523
        along_x_0 = (floor(at_1.x / size_3) + 1.0f) * size_3;

#line 523
    }
    else
    {

#line 523
        along_x_0 = floor(at_1.x / size_3) * size_3;

#line 523
    }
    float _S41 = forward_0.y;

#line 524
    bool _S42 = _S41 > 0.0f;

#line 524
    float along_y_0;

#line 524
    if(_S42)
    {

#line 524
        along_y_0 = (floor(at_1.y / size_3) + 1.0f) * size_3;

#line 524
    }
    else
    {

#line 524
        along_y_0 = floor(at_1.y / size_3) * size_3;

#line 524
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 525
    float _S43;

    if((abs(_S39)) < 9.99999997475242708e-07f)
    {

#line 527
        along_x_0 = reach_0;

#line 527
    }
    else
    {

#line 528
        if(_S40)
        {

#line 528
            _S43 = nudge_0;

#line 528
        }
        else
        {

#line 528
            _S43 = - nudge_0;

#line 528
        }

#line 528
        along_x_0 = (along_x_0 + _S43 - at_1.x) / _S39;

#line 527
    }


    if((abs(_S41)) < 9.99999997475242708e-07f)
    {

#line 530
        along_y_0 = reach_0;

#line 530
    }
    else
    {

#line 531
        if(_S42)
        {

#line 531
            _S43 = nudge_0;

#line 531
        }
        else
        {

#line 531
            _S43 = - nudge_0;

#line 531
        }

#line 531
        along_y_0 = (along_y_0 + _S43 - at_1.y) / _S41;

#line 530
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 479
float hiz_at_0(uint level_0, int2 texel_0, int2 extent_5, KernelContext_0 thread* kernelContext_6)
{
    int2 _S44 = int2(int(0), int(0));
    int3 at_2 = int3(clamp(texel_0, _S44, max(extent_5 - int2(int(1), int(1)), _S44)), int(0));
    switch(level_0)
    {
    case 0U:
        {

#line 486
            return ((kernelContext_6->scene_depth_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 1U:
        {

#line 488
            return ((kernelContext_6->hiz_1_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 2U:
        {

#line 490
            return ((kernelContext_6->hiz_2_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 3U:
        {

#line 492
            return ((kernelContext_6->hiz_3_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 4U:
        {

#line 494
            return ((kernelContext_6->hiz_4_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    default:
        {

#line 496
            return ((kernelContext_6->hiz_5_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    }

#line 496
}


#line 507
float view_z_of_0(float depth_2, KernelContext_0 thread* kernelContext_7)
{
    float4 view_2 = (((float4(0.0f, 0.0f, depth_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_7->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_7->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_7->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_7->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_7->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_7->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_7->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_7->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_7->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_7->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_7->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_7->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_7->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_7->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_7->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_7->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_2.z / view_2.w;
}


#line 462
float thickness_at_0(float advance_0, float depth_3)
{
    return max(advance_0, abs(depth_3) * 0.01999999955296516f);
}


#line 464
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 464
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 604
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S45 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 604
    float3 reflection_0;

#line 604
    thread KernelContext_0 kernelContext_8;

#line 604
    (&kernelContext_8)->scene_depth_0 = scene_depth_1;

#line 604
    (&kernelContext_8)->reflectivity_0 = reflectivity_1;

#line 604
    (&kernelContext_8)->camera_0 = camera_1;

#line 604
    (&kernelContext_8)->probes_0 = probes_1;

#line 604
    (&kernelContext_8)->hiz_1_0 = hiz_1_1;

#line 604
    (&kernelContext_8)->hiz_2_0 = hiz_2_1;

#line 604
    (&kernelContext_8)->hiz_3_0 = hiz_3_1;

#line 604
    (&kernelContext_8)->hiz_4_0 = hiz_4_1;

#line 604
    (&kernelContext_8)->hiz_5_0 = hiz_5_1;

#line 604
    (&kernelContext_8)->scene_color_0 = scene_color_1;

    thread uint width_0;
    thread uint height_0;



    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int _S46 = int(width_0);

#line 612
    int _S47 = int(height_0);

#line 612
    int2 extent_6 = int2(_S46, _S47);
    float _S48 = float(width_0);

#line 613
    float _S49 = float(height_0);

#line 613
    float2 size_4 = float2(_S48, _S49);
    int2 _S50 = int2(position_0.xy);

#line 621
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S51 = int3(_S50, int(0));

#line 623
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S51)).xy), uint(((_S51)).z)));
    float sharpness_0 = surface_0.w;

#line 624
    float _S52 = depth_at_0(_S50, extent_6, &kernelContext_8);


    if(_S52 <= 0.0f)
    {

#line 627
        pixelOutput_0 _S53 = { NOTHING_0 };

        return _S53;
    }

#line 629
    float3 _S54 = view_position_0(_S50, _S52, size_4, &kernelContext_8);

#line 629
    float3 _S55 = normal_at_0(_S50, _S54, extent_6, size_4, &kernelContext_8);

#line 635
    float3 towards_0 = normalize(_S54);
    float3 ray_0 = reflect(towards_0, _S55);


    float4 _S56 = float4(ray_0, 0.0f);

#line 639
    float3 _S57 = probe_environment_0((((float4(_S54, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_8)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, normalize((((_S56) * (matrix<float,int(4),int(4)> ((&kernelContext_8)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_8)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz), &kernelContext_8);

#line 645
    float3 _S58 = - towards_0;
    float3 f0_0 = surface_0.xyz;
    float grazing_0 = 1.0f - saturate(dot(_S55, _S58));
    float grazing2_0 = grazing_0 * grazing_0;
    float3 fresnel_0 = f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) ;

#line 654
    if(sharpness_0 <= 0.0f)
    {

#line 654
        pixelOutput_0 _S59 = { float4(_S57 * fresnel_0, 0.0f) };

        return _S59;
    }


    float _S60 = saturate((1.0f - dot(ray_0, _S58)) / 0.05000000074505806f);


    float _S61 = _S54.z;

#line 663
    float3 start_0 = _S54 + _S55 * float3((abs(_S61) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_8)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_8)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_8)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_8)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_8)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_8)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_8)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_8)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_8)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_8)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_8)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_8)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_8)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_8)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_8)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_8)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S56) * (matrix<float,int(4),int(4)> ((&kernelContext_8)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_8)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_8)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_8)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_8)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_8)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_8)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_8)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_8)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_8)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_8)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_8)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_8)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_8)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_8)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_8)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S62 = clip_start_0.w;

#line 668
    if(_S62 <= 0.0f)
    {

#line 668
        pixelOutput_0 _S63 = { float4(_S57 * fresnel_0, sharpness_0) };

        return _S63;
    }
    float2 _S64 = clip_start_0.xy;

#line 672
    float2 _S65 = float2(_S62) ;

#line 672
    float2 at_start_0 = pixel_of_0(_S64 / _S65, size_4);

#line 678
    float2 _S66 = clip_ray_0.xy;

#line 678
    float _S67 = clip_ray_0.w;

#line 678
    float2 _S68 = float2(_S67) ;

#line 678
    float2 ndc_rate_0 = (_S66 * _S65 - _S64 * _S68) / float2((_S62 * _S62)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S48, - ndc_rate_0.y * 0.5f * _S49);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 681
        pixelOutput_0 _S69 = { float4(_S57 * fresnel_0, sharpness_0) };

        return _S69;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 692
    float reach_1 = 0.75f * min(_S48, _S49);

    float _S70 = forward_1.x;

#line 694
    float travel_0;

#line 694
    if(_S70 > 0.0f)
    {

#line 694
        travel_0 = min(reach_1, (_S48 - 1.0f - at_start_0.x) / _S70);

#line 694
    }
    else
    {

        if(_S70 < 0.0f)
        {

#line 698
            travel_0 = min(reach_1, - at_start_0.x / _S70);

#line 698
        }
        else
        {

#line 698
            travel_0 = reach_1;

#line 698
        }

#line 694
    }

#line 702
    float _S71 = forward_1.y;

#line 702
    if(_S71 > 0.0f)
    {

#line 702
        travel_0 = min(travel_0, (_S49 - 1.0f - at_start_0.y) / _S71);

#line 702
    }
    else
    {

        if(_S71 < 0.0f)
        {

#line 706
            travel_0 = min(travel_0, - at_start_0.y / _S71);

#line 706
        }

#line 702
    }

#line 714
    if(_S67 > 0.0f)
    {

#line 714
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S66 / _S68, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 714
    }
    else
    {

#line 729
        if(_S67 < 0.0f)
        {

#line 736
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_8)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_8)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 741
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S62) / _S67)) ;

#line 741
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 729
        }

#line 714
    }

#line 748
    float _S72 = max(travel_0, 0.0f);
    if(_S72 <= 0.00390625f)
    {

#line 749
        pixelOutput_0 _S73 = { float4(_S57 * fresnel_0, sharpness_0) };

        return _S73;
    }

#line 758
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S72) , size_4);

#line 758
    float when_end_0;

    if((abs(_S70)) >= (abs(_S71)))
    {

#line 760
        float _S74 = ndc_end_0.x;

#line 760
        when_end_0 = (_S74 * _S62 - clip_start_0.x) / (clip_ray_0.x - _S74 * _S67);

#line 760
    }
    else
    {

#line 761
        float _S75 = ndc_end_0.y;

#line 761
        when_end_0 = (_S75 * _S62 - clip_start_0.y) / (clip_ray_0.y - _S75 * _S67);

#line 760
    }

#line 760
    bool _S76;

#line 768
    if(!(when_end_0 > 0.0f))
    {

#line 768
        _S76 = true;

#line 768
    }
    else
    {

#line 768
        _S76 = !isfinite(when_end_0);

#line 768
    }

#line 768
    if(_S76)
    {

#line 768
        pixelOutput_0 _S77 = { float4(_S57 * fresnel_0, sharpness_0) };

        return _S77;
    }

#line 776
    float inverse_w_start_0 = 1.0f / _S62;

    float inverse_w_end_0 = 1.0f / (_S62 + when_end_0 * _S67);
    float _S78 = start_0.z;

#line 779
    float _S79 = _S78 * inverse_w_start_0;
    float _S80 = (_S78 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 785
    float3 _S81 = _S57 * fresnel_0;
    uint _S82 = min((&kernelContext_8)->camera_0->hiz_0.x, 5U);

#line 816
    float _S83 = _S78 - _S61;

#line 816
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S72), _S72);

#line 816
    float previous_gap_0 = _S83;

#line 816
    float entry_z_0 = _S78;

#line 816
    uint step_0 = 0U;

#line 816
    uint level_1 = 0U;

    for(;;)
    {

#line 818
        if(step_0 < 96U)
        {
        }
        else
        {

#line 818
            reflection_0 = _S81;

#line 818
            break;
        }
        float cell_1 = float(1U << level_1);
        float2 at_3 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S84 = min(at_travel_0 + cell_exit_0(at_3, forward_1, cell_1, _S72), _S72);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S84) ;
        float along_0 = _S84 / _S72;

        float exit_z_0 = mix(_S79, _S80, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 826
        float _S85 = hiz_at_0(level_1, int2(floor(at_3 / float2(cell_1) )), int2(_S46 >> level_1, _S47 >> level_1), &kernelContext_8);

#line 826
        float gap_0;

#line 835
        if(_S85 <= 0.0f)
        {

#line 835
            gap_0 = 1.0f;

#line 835
        }
        else
        {

#line 835
            float _S86 = view_z_of_0(_S85, &kernelContext_8);

#line 835
            gap_0 = exit_z_0 - _S86;

#line 835
        }

#line 844
        bool _S87 = !(gap_0 > 0.0f);

#line 844
        if(_S87)
        {

#line 844
            _S76 = level_1 > 0U;

#line 844
        }
        else
        {

#line 844
            _S76 = false;

#line 844
        }

#line 844
        if(_S76)
        {

#line 844
            level_1 = level_1 - 1U;

#line 850
            step_0 = step_0 + 1U;

#line 818
            continue;
        }

#line 818
        bool _S88;

#line 853
        if(_S87)
        {

#line 853
            _S88 = previous_gap_0 > 0.0f;

#line 853
        }
        else
        {

#line 853
            _S88 = false;

#line 853
        }

#line 853
        if(_S88)
        {



            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {

#line 866
                float2 hit_at_0 = mix(at_3, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 881
                float confidence_0 = sharpness_0 * _S60 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S84 / reach_1) / 0.25f) * saturate(1.0f - behind_0 / thickness_0);
                int3 _S89 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_6 - int2(int(1), int(1))), int(0));

#line 882
                reflection_0 = (((&kernelContext_8)->scene_color_0).read(vec<uint,2>(((_S89)).xy), uint(((_S89)).z))).xyz * fresnel_0 * float3(confidence_0)  + _S81 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 853
        }

#line 894
        if(_S84 >= _S72)
        {

#line 894
            reflection_0 = _S81;

            break;
        }



        uint _S90 = min(level_1 + 1U, _S82);

#line 901
        at_travel_0 = _S84;

#line 901
        previous_gap_0 = gap_0;

#line 901
        entry_z_0 = exit_z_0;

#line 901
        level_1 = _S90;

#line 818
        step_0 = step_0 + 1U;

#line 818
    }

#line 818
    pixelOutput_0 _S91 = { float4(reflection_0, sharpness_0) };

#line 909
    return _S91;
}


#line 909
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 363
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 363
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 363
    thread KernelContext_0 kernelContext_9;

#line 363
    (&kernelContext_9)->scene_depth_0 = scene_depth_2;

#line 363
    (&kernelContext_9)->reflectivity_0 = reflectivity_2;

#line 363
    (&kernelContext_9)->camera_0 = camera_2;

#line 363
    (&kernelContext_9)->probes_0 = probes_2;

#line 363
    (&kernelContext_9)->hiz_1_0 = hiz_1_2;

#line 363
    (&kernelContext_9)->hiz_2_0 = hiz_2_2;

#line 363
    (&kernelContext_9)->hiz_3_0 = hiz_3_2;

#line 363
    (&kernelContext_9)->hiz_4_0 = hiz_4_2;

#line 363
    (&kernelContext_9)->hiz_5_0 = hiz_5_2;

#line 363
    (&kernelContext_9)->scene_color_0 = scene_color_2;

#line 595
    thread FullscreenOutput_0 output_1;


    float2 _S92 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 598
    (&output_1)->uv_2 = _S92;
    (&output_1)->position_2 = float4(_S92 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 599
    thread vertexMain_Result_0 _S93;

#line 599
    (&_S93)->position_1 = output_1.position_2;

#line 599
    (&_S93)->uv_1 = output_1.uv_2;

#line 599
    return _S93;
}

