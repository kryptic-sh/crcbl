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
    depth2d<float, access::sample> hiz_1_0;
    depth2d<float, access::sample> hiz_2_0;
    depth2d<float, access::sample> hiz_3_0;
    depth2d<float, access::sample> hiz_4_0;
    depth2d<float, access::sample> hiz_5_0;
    texture2d<float, access::sample> scene_color_0;
};


#line 391 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 394
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 391
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 394
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 403
float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 413
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 403
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 413
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 429
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 431
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 431
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 432
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 432
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 433
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 433
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 434
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 434
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 436
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 437
        horizontal_0 = _S8 - centre_0;

#line 437
    }
    else
    {

#line 437
        horizontal_0 = centre_0 - _S5;

#line 437
    }

#line 437
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 440
        vertical_0 = _S14 - centre_0;

#line 440
    }
    else
    {

#line 440
        vertical_0 = centre_0 - _S11;

#line 440
    }

#line 450
    return normalize(cross(vertical_0, horizontal_0));
}


#line 138
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 584
float3 probe_environment_0(float3 world_position_0, float3 direction_0, KernelContext_0 thread* kernelContext_5)
{

#line 584
    float3 _S16 = float3(1.0f) ;

    float3 _S17 = float3(0.0f, 0.0f, 0.0f);

#line 586
    float3 last_0 = max(float3(kernelContext_5->camera_0->probe_counts_0.xyz) - _S16, _S17);
    float3 grid_0 = clamp((world_position_0 - kernelContext_5->camera_0->probe_origin_0.xyz) * kernelContext_5->camera_0->probe_inv_spacing_0.xyz, _S17, last_0);

    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S18 = uint3(base_0);
    uint3 _S19 = uint3(min(base_0 + _S16, last_0));
    uint total_0 = max(kernelContext_5->camera_0->probe_counts_0.w, 1U) - 1U;
    uint _S20 = _S18.z;

#line 594
    uint _S21 = _S18.y;

#line 594
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

#line 611
    float4 _S27 = float4(f_0.y) ;

#line 611
    float4 _S28 = mix(mix(float4(x00_0.sh_r_0) , float4(y00_0.sh_r_0) , _S26), mix(float4(x10_0.sh_r_0) , float4(y10_0.sh_r_0) , _S26), _S27);

#line 611
    (&z0_0)->sh_r_0 = _S28;
    float4 _S29 = mix(mix(float4(x00_0.sh_g_0) , float4(y00_0.sh_g_0) , _S26), mix(float4(x10_0.sh_g_0) , float4(y10_0.sh_g_0) , _S26), _S27);

#line 612
    (&z0_0)->sh_g_0 = _S29;
    float4 _S30 = mix(mix(float4(x00_0.sh_b_0) , float4(y00_0.sh_b_0) , _S26), mix(float4(x10_0.sh_b_0) , float4(y10_0.sh_b_0) , _S26), _S27);

#line 613
    (&z0_0)->sh_b_0 = _S30;
    thread GpuProbe_0 z1_0;
    float4 _S31 = mix(mix(float4(x01_0.sh_r_0) , float4(y01_0.sh_r_0) , _S26), mix(float4(x11_0.sh_r_0) , float4(y11_0.sh_r_0) , _S26), _S27);

#line 615
    (&z1_0)->sh_r_0 = _S31;
    float4 _S32 = mix(mix(float4(x01_0.sh_g_0) , float4(y01_0.sh_g_0) , _S26), mix(float4(x11_0.sh_g_0) , float4(y11_0.sh_g_0) , _S26), _S27);

#line 616
    (&z1_0)->sh_g_0 = _S32;
    float4 _S33 = mix(mix(float4(x01_0.sh_b_0) , float4(y01_0.sh_b_0) , _S26), mix(float4(x11_0.sh_b_0) , float4(y11_0.sh_b_0) , _S26), _S27);

#line 617
    (&z1_0)->sh_b_0 = _S33;
    thread GpuProbe_0 cell_0;
    float4 _S34 = float4(f_0.z) ;

#line 619
    float4 _S35 = mix(_S28, _S31, _S34);

#line 619
    (&cell_0)->sh_r_0 = _S35;
    float4 _S36 = mix(_S29, _S32, _S34);

#line 620
    (&cell_0)->sh_g_0 = _S36;
    float4 _S37 = mix(_S30, _S33, _S34);

#line 621
    (&cell_0)->sh_b_0 = _S37;

#line 621
    float3 _S38 = float3(2.09439516067504883f) ;
    return max(float3(dot(_S35.xyz / _S38, direction_0) + _S35.w / 3.14159274101257324f, dot(_S36.xyz / _S38, direction_0) + _S36.w / 3.14159274101257324f, dot(_S37.xyz / _S38, direction_0) + _S37.w / 3.14159274101257324f), _S17);
}


#line 575
float3 sky_radiance_0(float3 direction_1, KernelContext_0 thread* kernelContext_6)
{
    float up_0 = clamp(direction_1.y, -1.0f, 1.0f);

#line 577
    float3 far_0;
    if(up_0 >= 0.0f)
    {

#line 578
        far_0 = kernelContext_6->camera_0->sky_0[int(0)].xyz;

#line 578
    }
    else
    {

#line 578
        far_0 = kernelContext_6->camera_0->sky_0[int(2)].xyz;

#line 578
    }
    float u_0 = abs(up_0);
    float blend_0 = u_0 * u_0 * (3.0f - 2.0f * u_0);
    return kernelContext_6->camera_0->sky_0[int(1)].xyz * float3((1.0f - blend_0))  + far_0 * float3(blend_0) ;
}


#line 459
float2 pixel_of_0(float2 ndc_0, float2 size_1)
{
    return float2((ndc_0.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_0.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_0, float2 size_2)
{
    return float2(at_0.x / size_2.x * 2.0f - 1.0f, 1.0f - at_0.y / size_2.y * 2.0f);
}


#line 536
float cell_exit_0(float2 at_1, float2 forward_0, float size_3, float reach_0)
{

    float _S39 = forward_0.x;

#line 539
    bool _S40 = _S39 > 0.0f;

#line 539
    float along_x_0;

#line 539
    if(_S40)
    {

#line 539
        along_x_0 = (floor(at_1.x / size_3) + 1.0f) * size_3;

#line 539
    }
    else
    {

#line 539
        along_x_0 = floor(at_1.x / size_3) * size_3;

#line 539
    }
    float _S41 = forward_0.y;

#line 540
    bool _S42 = _S41 > 0.0f;

#line 540
    float along_y_0;

#line 540
    if(_S42)
    {

#line 540
        along_y_0 = (floor(at_1.y / size_3) + 1.0f) * size_3;

#line 540
    }
    else
    {

#line 540
        along_y_0 = floor(at_1.y / size_3) * size_3;

#line 540
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 541
    float _S43;

    if((abs(_S39)) < 9.99999997475242708e-07f)
    {

#line 543
        along_x_0 = reach_0;

#line 543
    }
    else
    {

#line 544
        if(_S40)
        {

#line 544
            _S43 = nudge_0;

#line 544
        }
        else
        {

#line 544
            _S43 = - nudge_0;

#line 544
        }

#line 544
        along_x_0 = (along_x_0 + _S43 - at_1.x) / _S39;

#line 543
    }


    if((abs(_S41)) < 9.99999997475242708e-07f)
    {

#line 546
        along_y_0 = reach_0;

#line 546
    }
    else
    {

#line 547
        if(_S42)
        {

#line 547
            _S43 = nudge_0;

#line 547
        }
        else
        {

#line 547
            _S43 = - nudge_0;

#line 547
        }

#line 547
        along_y_0 = (along_y_0 + _S43 - at_1.y) / _S41;

#line 546
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 495
float hiz_at_0(uint level_0, int2 texel_0, int2 extent_5, KernelContext_0 thread* kernelContext_7)
{
    int2 _S44 = int2(int(0), int(0));
    int3 at_2 = int3(clamp(texel_0, _S44, max(extent_5 - int2(int(1), int(1)), _S44)), int(0));
    switch(level_0)
    {
    case 0U:
        {

#line 502
            return ((kernelContext_7->scene_depth_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 1U:
        {

#line 504
            return ((kernelContext_7->hiz_1_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 2U:
        {

#line 506
            return ((kernelContext_7->hiz_2_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 3U:
        {

#line 508
            return ((kernelContext_7->hiz_3_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    case 4U:
        {

#line 510
            return ((kernelContext_7->hiz_4_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    default:
        {

#line 512
            return ((kernelContext_7->hiz_5_0).read(vec<uint,2>(((at_2)).xy), uint(((at_2)).z)));
        }
    }

#line 512
}


#line 523
float view_z_of_0(float depth_2, KernelContext_0 thread* kernelContext_8)
{
    float4 view_2 = (((float4(0.0f, 0.0f, depth_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_8->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_8->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_8->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_8->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_8->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_8->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_8->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_8->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_8->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_8->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_8->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_8->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_8->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_8->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_8->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_8->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_2.z / view_2.w;
}


#line 478
float thickness_at_0(float advance_0, float depth_3)
{
    return max(advance_0, abs(depth_3) * 0.01999999955296516f);
}


#line 480
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 480
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 641
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S45 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], depth2d<float, access::sample> hiz_1_1 [[texture(3)]], depth2d<float, access::sample> hiz_2_1 [[texture(4)]], depth2d<float, access::sample> hiz_3_1 [[texture(5)]], depth2d<float, access::sample> hiz_4_1 [[texture(6)]], depth2d<float, access::sample> hiz_5_1 [[texture(7)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 641
    float3 reflection_0;

#line 641
    thread KernelContext_0 kernelContext_9;

#line 641
    (&kernelContext_9)->scene_depth_0 = scene_depth_1;

#line 641
    (&kernelContext_9)->reflectivity_0 = reflectivity_1;

#line 641
    (&kernelContext_9)->camera_0 = camera_1;

#line 641
    (&kernelContext_9)->probes_0 = probes_1;

#line 641
    (&kernelContext_9)->hiz_1_0 = hiz_1_1;

#line 641
    (&kernelContext_9)->hiz_2_0 = hiz_2_1;

#line 641
    (&kernelContext_9)->hiz_3_0 = hiz_3_1;

#line 641
    (&kernelContext_9)->hiz_4_0 = hiz_4_1;

#line 641
    (&kernelContext_9)->hiz_5_0 = hiz_5_1;

#line 641
    (&kernelContext_9)->scene_color_0 = scene_color_1;

    thread uint width_0;
    thread uint height_0;



    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int _S46 = int(width_0);

#line 649
    int _S47 = int(height_0);

#line 649
    int2 extent_6 = int2(_S46, _S47);
    float _S48 = float(width_0);

#line 650
    float _S49 = float(height_0);

#line 650
    float2 size_4 = float2(_S48, _S49);
    int2 _S50 = int2(position_0.xy);

#line 658
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S51 = int3(_S50, int(0));

#line 660
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S51)).xy), uint(((_S51)).z)));
    float sharpness_0 = surface_0.w;

#line 661
    float _S52 = depth_at_0(_S50, extent_6, &kernelContext_9);


    if(_S52 <= 0.0f)
    {

#line 664
        pixelOutput_0 _S53 = { NOTHING_0 };

        return _S53;
    }

#line 666
    float3 _S54 = view_position_0(_S50, _S52, size_4, &kernelContext_9);

#line 666
    float3 _S55 = normal_at_0(_S50, _S54, extent_6, size_4, &kernelContext_9);

#line 672
    float3 towards_0 = normalize(_S54);
    float3 ray_0 = reflect(towards_0, _S55);


    float4 _S56 = float4(ray_0, 0.0f);

#line 676
    float3 reflection_direction_0 = normalize((((_S56) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 676
    float3 _S57 = probe_environment_0((((float4(_S54, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_9)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, reflection_direction_0, &kernelContext_9);

#line 676
    float3 _S58 = sky_radiance_0(reflection_direction_0, &kernelContext_9);

#line 684
    float3 environment_0 = _S57 + _S58;

#line 689
    float3 _S59 = - towards_0;
    float3 f0_0 = surface_0.xyz;
    float grazing_0 = 1.0f - saturate(dot(_S55, _S59));
    float grazing2_0 = grazing_0 * grazing_0;
    float3 fresnel_0 = f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) ;

#line 698
    if(sharpness_0 <= 0.0f)
    {

#line 698
        pixelOutput_0 _S60 = { float4(environment_0 * fresnel_0, 0.0f) };

        return _S60;
    }


    float _S61 = saturate((1.0f - dot(ray_0, _S59)) / 0.05000000074505806f);


    float _S62 = _S54.z;

#line 707
    float3 start_0 = _S54 + _S55 * float3((abs(_S62) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S56) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_9)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_9)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S63 = clip_start_0.w;

#line 712
    if(_S63 <= 0.0f)
    {

#line 712
        pixelOutput_0 _S64 = { float4(environment_0 * fresnel_0, sharpness_0) };

        return _S64;
    }
    float2 _S65 = clip_start_0.xy;

#line 716
    float2 _S66 = float2(_S63) ;

#line 716
    float2 at_start_0 = pixel_of_0(_S65 / _S66, size_4);

#line 722
    float2 _S67 = clip_ray_0.xy;

#line 722
    float _S68 = clip_ray_0.w;

#line 722
    float2 _S69 = float2(_S68) ;

#line 722
    float2 ndc_rate_0 = (_S67 * _S66 - _S65 * _S69) / float2((_S63 * _S63)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S48, - ndc_rate_0.y * 0.5f * _S49);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 725
        pixelOutput_0 _S70 = { float4(environment_0 * fresnel_0, sharpness_0) };

        return _S70;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 736
    float reach_1 = 0.75f * min(_S48, _S49);

    float _S71 = forward_1.x;

#line 738
    float travel_0;

#line 738
    if(_S71 > 0.0f)
    {

#line 738
        travel_0 = min(reach_1, (_S48 - 1.0f - at_start_0.x) / _S71);

#line 738
    }
    else
    {

        if(_S71 < 0.0f)
        {

#line 742
            travel_0 = min(reach_1, - at_start_0.x / _S71);

#line 742
        }
        else
        {

#line 742
            travel_0 = reach_1;

#line 742
        }

#line 738
    }

#line 746
    float _S72 = forward_1.y;

#line 746
    if(_S72 > 0.0f)
    {

#line 746
        travel_0 = min(travel_0, (_S49 - 1.0f - at_start_0.y) / _S72);

#line 746
    }
    else
    {

        if(_S72 < 0.0f)
        {

#line 750
            travel_0 = min(travel_0, - at_start_0.y / _S72);

#line 750
        }

#line 746
    }

#line 758
    if(_S68 > 0.0f)
    {

#line 758
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S67 / _S69, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));

#line 758
    }
    else
    {

#line 773
        if(_S68 < 0.0f)
        {

#line 780
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_9)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 785
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S63) / _S68)) ;

#line 785
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 773
        }

#line 758
    }

#line 792
    float _S73 = max(travel_0, 0.0f);
    if(_S73 <= 0.00390625f)
    {

#line 793
        pixelOutput_0 _S74 = { float4(environment_0 * fresnel_0, sharpness_0) };

        return _S74;
    }

#line 802
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S73) , size_4);

#line 802
    float when_end_0;

    if((abs(_S71)) >= (abs(_S72)))
    {

#line 804
        float _S75 = ndc_end_0.x;

#line 804
        when_end_0 = (_S75 * _S63 - clip_start_0.x) / (clip_ray_0.x - _S75 * _S68);

#line 804
    }
    else
    {

#line 805
        float _S76 = ndc_end_0.y;

#line 805
        when_end_0 = (_S76 * _S63 - clip_start_0.y) / (clip_ray_0.y - _S76 * _S68);

#line 804
    }

#line 804
    bool _S77;

#line 812
    if(!(when_end_0 > 0.0f))
    {

#line 812
        _S77 = true;

#line 812
    }
    else
    {

#line 812
        _S77 = !isfinite(when_end_0);

#line 812
    }

#line 812
    if(_S77)
    {

#line 812
        pixelOutput_0 _S78 = { float4(environment_0 * fresnel_0, sharpness_0) };

        return _S78;
    }

#line 820
    float inverse_w_start_0 = 1.0f / _S63;

    float inverse_w_end_0 = 1.0f / (_S63 + when_end_0 * _S68);
    float _S79 = start_0.z;

#line 823
    float _S80 = _S79 * inverse_w_start_0;
    float _S81 = (_S79 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 829
    float3 _S82 = environment_0 * fresnel_0;
    uint _S83 = min((&kernelContext_9)->camera_0->hiz_0.x, 5U);

#line 860
    float _S84 = _S79 - _S62;

#line 860
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S73), _S73);

#line 860
    float previous_gap_0 = _S84;

#line 860
    float entry_z_0 = _S79;

#line 860
    uint step_0 = 0U;

#line 860
    uint level_1 = 0U;

    for(;;)
    {

#line 862
        if(step_0 < 96U)
        {
        }
        else
        {

#line 862
            reflection_0 = _S82;

#line 862
            break;
        }
        float cell_1 = float(1U << level_1);
        float2 at_3 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S85 = min(at_travel_0 + cell_exit_0(at_3, forward_1, cell_1, _S73), _S73);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S85) ;
        float along_0 = _S85 / _S73;

        float exit_z_0 = mix(_S80, _S81, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 870
        float _S86 = hiz_at_0(level_1, int2(floor(at_3 / float2(cell_1) )), int2(_S46 >> level_1, _S47 >> level_1), &kernelContext_9);

#line 870
        float gap_0;

#line 879
        if(_S86 <= 0.0f)
        {

#line 879
            gap_0 = 1.0f;

#line 879
        }
        else
        {

#line 879
            float _S87 = view_z_of_0(_S86, &kernelContext_9);

#line 879
            gap_0 = exit_z_0 - _S87;

#line 879
        }

#line 888
        bool _S88 = !(gap_0 > 0.0f);

#line 888
        if(_S88)
        {

#line 888
            _S77 = level_1 > 0U;

#line 888
        }
        else
        {

#line 888
            _S77 = false;

#line 888
        }

#line 888
        if(_S77)
        {

#line 888
            level_1 = level_1 - 1U;

#line 894
            step_0 = step_0 + 1U;

#line 862
            continue;
        }

#line 862
        bool _S89;

#line 897
        if(_S88)
        {

#line 897
            _S89 = previous_gap_0 > 0.0f;

#line 897
        }
        else
        {

#line 897
            _S89 = false;

#line 897
        }

#line 897
        if(_S89)
        {



            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {

#line 910
                float2 hit_at_0 = mix(at_3, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_4);

#line 925
                float confidence_0 = sharpness_0 * _S61 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S85 / reach_1) / 0.25f) * saturate(1.0f - behind_0 / thickness_0);
                int3 _S90 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_6 - int2(int(1), int(1))), int(0));

#line 926
                reflection_0 = (((&kernelContext_9)->scene_color_0).read(vec<uint,2>(((_S90)).xy), uint(((_S90)).z))).xyz * fresnel_0 * float3(confidence_0)  + _S82 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 897
        }

#line 938
        if(_S85 >= _S73)
        {

#line 938
            reflection_0 = _S82;

            break;
        }



        uint _S91 = min(level_1 + 1U, _S83);

#line 945
        at_travel_0 = _S85;

#line 945
        previous_gap_0 = gap_0;

#line 945
        entry_z_0 = exit_z_0;

#line 945
        level_1 = _S91;

#line 862
        step_0 = step_0 + 1U;

#line 862
    }

#line 862
    pixelOutput_0 _S92 = { float4(reflection_0, sharpness_0) };

#line 953
    return _S92;
}


#line 953
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 379
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 379
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], depth2d<float, access::sample> hiz_1_2 [[texture(3)]], depth2d<float, access::sample> hiz_2_2 [[texture(4)]], depth2d<float, access::sample> hiz_3_2 [[texture(5)]], depth2d<float, access::sample> hiz_4_2 [[texture(6)]], depth2d<float, access::sample> hiz_5_2 [[texture(7)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 379
    thread KernelContext_0 kernelContext_10;

#line 379
    (&kernelContext_10)->scene_depth_0 = scene_depth_2;

#line 379
    (&kernelContext_10)->reflectivity_0 = reflectivity_2;

#line 379
    (&kernelContext_10)->camera_0 = camera_2;

#line 379
    (&kernelContext_10)->probes_0 = probes_2;

#line 379
    (&kernelContext_10)->hiz_1_0 = hiz_1_2;

#line 379
    (&kernelContext_10)->hiz_2_0 = hiz_2_2;

#line 379
    (&kernelContext_10)->hiz_3_0 = hiz_3_2;

#line 379
    (&kernelContext_10)->hiz_4_0 = hiz_4_2;

#line 379
    (&kernelContext_10)->hiz_5_0 = hiz_5_2;

#line 379
    (&kernelContext_10)->scene_color_0 = scene_color_2;

#line 632
    thread FullscreenOutput_0 output_1;


    float2 _S93 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 635
    (&output_1)->uv_2 = _S93;
    (&output_1)->position_2 = float4(_S93 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 636
    thread vertexMain_Result_0 _S94;

#line 636
    (&_S94)->position_1 = output_1.position_2;

#line 636
    (&_S94)->uv_1 = output_1.uv_2;

#line 636
    return _S94;
}

