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
    texture2d<float, access::sample> scene_color_0;
};


#line 294 "shaders/ssr.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 297
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 294
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 297
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 306
float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 316
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 306
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 316
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 332
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 334
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 334
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 335
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 335
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 336
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 336
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 337
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 337
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 339
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 340
        horizontal_0 = _S8 - centre_0;

#line 340
    }
    else
    {

#line 340
        horizontal_0 = centre_0 - _S5;

#line 340
    }

#line 340
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 343
        vertical_0 = _S14 - centre_0;

#line 343
    }
    else
    {

#line 343
        vertical_0 = centre_0 - _S11;

#line 343
    }

#line 353
    return normalize(cross(vertical_0, horizontal_0));
}


#line 114
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 398
float3 probe_environment_0(float3 world_position_0, float3 direction_0, KernelContext_0 thread* kernelContext_5)
{

#line 398
    float3 _S16 = float3(1.0f) ;

    float3 _S17 = float3(0.0f, 0.0f, 0.0f);

#line 400
    float3 last_0 = max(float3(kernelContext_5->camera_0->probe_counts_0.xyz) - _S16, _S17);
    float3 grid_0 = clamp((world_position_0 - kernelContext_5->camera_0->probe_origin_0.xyz) * kernelContext_5->camera_0->probe_inv_spacing_0.xyz, _S17, last_0);

    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S18 = uint3(base_0);
    uint3 _S19 = uint3(min(base_0 + _S16, last_0));
    uint total_0 = max(kernelContext_5->camera_0->probe_counts_0.w, 1U) - 1U;
    uint _S20 = _S18.z;

#line 408
    uint _S21 = _S18.y;

#line 408
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

#line 425
    float4 _S27 = float4(f_0.y) ;

#line 425
    float4 _S28 = mix(mix(float4(x00_0.sh_r_0) , float4(y00_0.sh_r_0) , _S26), mix(float4(x10_0.sh_r_0) , float4(y10_0.sh_r_0) , _S26), _S27);

#line 425
    (&z0_0)->sh_r_0 = _S28;
    float4 _S29 = mix(mix(float4(x00_0.sh_g_0) , float4(y00_0.sh_g_0) , _S26), mix(float4(x10_0.sh_g_0) , float4(y10_0.sh_g_0) , _S26), _S27);

#line 426
    (&z0_0)->sh_g_0 = _S29;
    float4 _S30 = mix(mix(float4(x00_0.sh_b_0) , float4(y00_0.sh_b_0) , _S26), mix(float4(x10_0.sh_b_0) , float4(y10_0.sh_b_0) , _S26), _S27);

#line 427
    (&z0_0)->sh_b_0 = _S30;
    thread GpuProbe_0 z1_0;
    float4 _S31 = mix(mix(float4(x01_0.sh_r_0) , float4(y01_0.sh_r_0) , _S26), mix(float4(x11_0.sh_r_0) , float4(y11_0.sh_r_0) , _S26), _S27);

#line 429
    (&z1_0)->sh_r_0 = _S31;
    float4 _S32 = mix(mix(float4(x01_0.sh_g_0) , float4(y01_0.sh_g_0) , _S26), mix(float4(x11_0.sh_g_0) , float4(y11_0.sh_g_0) , _S26), _S27);

#line 430
    (&z1_0)->sh_g_0 = _S32;
    float4 _S33 = mix(mix(float4(x01_0.sh_b_0) , float4(y01_0.sh_b_0) , _S26), mix(float4(x11_0.sh_b_0) , float4(y11_0.sh_b_0) , _S26), _S27);

#line 431
    (&z1_0)->sh_b_0 = _S33;
    thread GpuProbe_0 cell_0;
    float4 _S34 = float4(f_0.z) ;

#line 433
    float4 _S35 = mix(_S28, _S31, _S34);

#line 433
    (&cell_0)->sh_r_0 = _S35;
    float4 _S36 = mix(_S29, _S32, _S34);

#line 434
    (&cell_0)->sh_g_0 = _S36;
    float4 _S37 = mix(_S30, _S33, _S34);

#line 435
    (&cell_0)->sh_b_0 = _S37;

#line 435
    float3 _S38 = float3(2.09439516067504883f) ;
    return max(float3(dot(_S35.xyz / _S38, direction_0) + _S35.w / 3.14159274101257324f, dot(_S36.xyz / _S38, direction_0) + _S36.w / 3.14159274101257324f, dot(_S37.xyz / _S38, direction_0) + _S37.w / 3.14159274101257324f), _S17);
}


#line 362
float2 pixel_of_0(float2 ndc_0, float2 size_1)
{
    return float2((ndc_0.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_0.y * 0.5f) * size_1.y);
}


float2 ndc_of_0(float2 at_0, float2 size_2)
{
    return float2(at_0.x / size_2.x * 2.0f - 1.0f, 1.0f - at_0.y / size_2.y * 2.0f);
}


#line 381
float thickness_at_0(float advance_0, float depth_2)
{
    return max(advance_0, abs(depth_2) * 0.01999999955296516f);
}


#line 383
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 383
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 455
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S39 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflectivity_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]], GpuProbe_natural_0 device* probes_1 [[buffer(1)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]])
{

#line 455
    float3 reflection_0;

#line 455
    thread KernelContext_0 kernelContext_6;

#line 455
    (&kernelContext_6)->scene_depth_0 = scene_depth_1;

#line 455
    (&kernelContext_6)->reflectivity_0 = reflectivity_1;

#line 455
    (&kernelContext_6)->camera_0 = camera_1;

#line 455
    (&kernelContext_6)->probes_0 = probes_1;

#line 455
    (&kernelContext_6)->scene_color_0 = scene_color_1;

    thread uint width_0;
    thread uint height_0;



    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_5 = int2(int(width_0), int(height_0));
    float _S40 = float(width_0);

#line 464
    float _S41 = float(height_0);

#line 464
    float2 size_3 = float2(_S40, _S41);
    int2 _S42 = int2(position_0.xy);

#line 472
    float4 NOTHING_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);

    int3 _S43 = int3(_S42, int(0));

#line 474
    float4 surface_0 = ((reflectivity_1).read(vec<uint,2>(((_S43)).xy), uint(((_S43)).z)));
    float sharpness_0 = surface_0.w;

#line 475
    float _S44 = depth_at_0(_S42, extent_5, &kernelContext_6);


    if(_S44 <= 0.0f)
    {

#line 478
        pixelOutput_0 _S45 = { NOTHING_0 };

        return _S45;
    }

#line 480
    float3 _S46 = view_position_0(_S42, _S44, size_3, &kernelContext_6);

#line 480
    float3 _S47 = normal_at_0(_S42, _S46, extent_5, size_3, &kernelContext_6);

#line 486
    float3 towards_0 = normalize(_S46);
    float3 ray_0 = reflect(towards_0, _S47);


    float4 _S48 = float4(ray_0, 0.0f);

#line 490
    float3 _S49 = probe_environment_0((((float4(_S46, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz, normalize((((_S48) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->camera_0->inv_view_0.data_0[int(0)][int(0)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(1)][int(0)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(2)][int(0)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(3)][int(0)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(0)][int(1)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(1)][int(1)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(2)][int(1)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(3)][int(1)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(0)][int(2)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(1)][int(2)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(2)][int(2)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(3)][int(2)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(0)][int(3)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(1)][int(3)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(2)][int(3)], (&kernelContext_6)->camera_0->inv_view_0.data_0[int(3)][int(3)])))).xyz), &kernelContext_6);

#line 496
    float3 _S50 = - towards_0;
    float3 f0_0 = surface_0.xyz;
    float grazing_0 = 1.0f - saturate(dot(_S47, _S50));
    float grazing2_0 = grazing_0 * grazing_0;
    float3 fresnel_0 = f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) ;

#line 505
    if(sharpness_0 <= 0.0f)
    {

#line 505
        pixelOutput_0 _S51 = { float4(_S49 * fresnel_0, 0.0f) };

        return _S51;
    }


    float _S52 = saturate((1.0f - dot(ray_0, _S50)) / 0.05000000074505806f);


    float _S53 = _S46.z;

#line 514
    float3 start_0 = _S46 + _S47 * float3((abs(_S53) * 0.00499999988824129f)) ;


    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((_S48) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S54 = clip_start_0.w;

#line 519
    if(_S54 <= 0.0f)
    {

#line 519
        pixelOutput_0 _S55 = { float4(_S49 * fresnel_0, sharpness_0) };

        return _S55;
    }
    float2 _S56 = clip_start_0.xy;

#line 523
    float2 _S57 = float2(_S54) ;

#line 523
    float2 at_start_0 = pixel_of_0(_S56 / _S57, size_3);

#line 529
    float2 _S58 = clip_ray_0.xy;

#line 529
    float _S59 = clip_ray_0.w;

#line 529
    float2 _S60 = float2(_S59) ;

#line 529
    float2 ndc_rate_0 = (_S58 * _S57 - _S56 * _S60) / float2((_S54 * _S54)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S40, - ndc_rate_0.y * 0.5f * _S41);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 532
        pixelOutput_0 _S61 = { float4(_S49 * fresnel_0, sharpness_0) };

        return _S61;
    }
    float2 forward_0 = screen_rate_0 / float2(rate_0) ;

#line 543
    float stride_0 = 0.75f * min(_S40, _S41) / 96.0f;
    float travel_0 = 96.0f * stride_0;
    float _S62 = forward_0.x;

#line 545
    float travel_1;

#line 545
    if(_S62 > 0.0f)
    {

#line 545
        travel_1 = min(travel_0, (_S40 - 1.0f - at_start_0.x) / _S62);

#line 545
    }
    else
    {

        if(_S62 < 0.0f)
        {

#line 549
            travel_1 = min(travel_0, - at_start_0.x / _S62);

#line 549
        }
        else
        {

#line 549
            travel_1 = travel_0;

#line 549
        }

#line 545
    }

#line 553
    float _S63 = forward_0.y;

#line 553
    if(_S63 > 0.0f)
    {

#line 553
        travel_1 = min(travel_1, (_S41 - 1.0f - at_start_0.y) / _S63);

#line 553
    }
    else
    {

        if(_S63 < 0.0f)
        {

#line 557
            travel_1 = min(travel_1, - at_start_0.y / _S63);

#line 557
        }

#line 553
    }

#line 565
    if(_S59 > 0.0f)
    {

#line 565
        travel_1 = min(travel_1, max(dot(pixel_of_0(_S58 / _S60, size_3) - at_start_0, forward_0), 0.0f));

#line 565
    }
    else
    {

#line 578
        if(_S59 < 0.0f)
        {

#line 585
            float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));

#line 590
            float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S54) / _S59)) ;

#line 590
            travel_1 = min(travel_1, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_3) - at_start_0, forward_0), 0.0f));

#line 578
        }

#line 565
    }

#line 597
    uint steps_0 = uint(max(travel_1, 0.0f) / stride_0);
    if(steps_0 == 0U)
    {

#line 598
        pixelOutput_0 _S64 = { float4(_S49 * fresnel_0, sharpness_0) };

        return _S64;
    }
    float _S65 = float(steps_0);

#line 602
    float travel_2 = _S65 * stride_0;

#line 608
    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_0 * float2(travel_2) , size_3);

#line 608
    float when_end_0;

    if((abs(_S62)) >= (abs(_S63)))
    {

#line 610
        float _S66 = ndc_end_0.x;

#line 610
        when_end_0 = (_S66 * _S54 - clip_start_0.x) / (clip_ray_0.x - _S66 * _S59);

#line 610
    }
    else
    {

#line 611
        float _S67 = ndc_end_0.y;

#line 611
        when_end_0 = (_S67 * _S54 - clip_start_0.y) / (clip_ray_0.y - _S67 * _S59);

#line 610
    }

#line 618
    if(!(when_end_0 > 0.0f))
    {

#line 618
        pixelOutput_0 _S68 = { float4(_S49 * fresnel_0, sharpness_0) };

        return _S68;
    }

#line 626
    float inverse_w_start_0 = 1.0f / _S54;

    float inverse_w_end_0 = 1.0f / (_S54 + when_end_0 * _S59);
    float _S69 = start_0.z;

#line 629
    float _S70 = _S69 * inverse_w_start_0;
    float _S71 = (_S69 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 639
    float3 _S72 = _S49 * fresnel_0;

#line 639
    float previous_gap_0 = _S69 - _S53;

#line 639
    float previous_z_0 = _S69;

#line 639
    float2 previous_at_0 = at_start_0;

#line 639
    uint step_0 = 1U;
    for(;;)
    {

#line 640
        if(step_0 <= steps_0)
        {
        }
        else
        {

#line 640
            reflection_0 = _S72;

#line 640
            break;
        }
        float _S73 = float(step_0);

#line 642
        float along_0 = _S73 / _S65;
        float2 at_1 = at_start_0 + forward_0 * float2((travel_2 * along_0)) ;
        int2 _S74 = int2(at_1);
        float ray_z_0 = mix(_S70, _S71, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 645
        float _S75 = depth_at_0(_S74, extent_5, &kernelContext_6);

#line 645
        float gap_0;

#line 652
        if(_S75 > 0.0f)
        {

#line 652
            float3 _S76 = view_position_0(_S74, _S75, size_3, &kernelContext_6);

#line 652
            gap_0 = ray_z_0 - _S76.z;

#line 652
        }
        else
        {

#line 652
            gap_0 = 1.0f;

#line 652
        }

#line 652
        bool _S77;

#line 660
        if(previous_gap_0 > 0.0f)
        {

#line 660
            _S77 = gap_0 < 0.0f;

#line 660
        }
        else
        {

#line 660
            _S77 = false;

#line 660
        }

#line 660
        if(_S77)
        {
            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(ray_z_0 - previous_z_0), ray_z_0);
            if(behind_0 <= thickness_0)
            {

#line 670
                float2 hit_at_0 = mix(previous_at_0, at_1, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) );


                float2 hit_ndc_0 = ndc_of_0(hit_at_0, size_3);

#line 685
                float confidence_0 = sharpness_0 * _S52 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S73 / 96.0f) / 0.25f) * saturate(1.0f - behind_0 / thickness_0);
                int3 _S78 = int3(clamp(int2(hit_at_0), int2(int(0), int(0)), extent_5 - int2(int(1), int(1))), int(0));

#line 686
                reflection_0 = (((&kernelContext_6)->scene_color_0).read(vec<uint,2>(((_S78)).xy), uint(((_S78)).z))).xyz * fresnel_0 * float3(confidence_0)  + _S72 * float3((1.0f - confidence_0)) ;


                break;
            }

#line 660
        }

#line 640
        uint step_1 = step_0 + 1U;

#line 640
        previous_gap_0 = gap_0;

#line 640
        previous_z_0 = ray_z_0;

#line 640
        previous_at_0 = at_1;

#line 640
        step_0 = step_1;

#line 640
    }

#line 640
    pixelOutput_0 _S79 = { float4(reflection_0, sharpness_0) };

#line 704
    return _S79;
}


#line 704
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 282
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 282
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflectivity_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]], GpuProbe_natural_0 device* probes_2 [[buffer(1)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]])
{

#line 282
    thread KernelContext_0 kernelContext_7;

#line 282
    (&kernelContext_7)->scene_depth_0 = scene_depth_2;

#line 282
    (&kernelContext_7)->reflectivity_0 = reflectivity_2;

#line 282
    (&kernelContext_7)->camera_0 = camera_2;

#line 282
    (&kernelContext_7)->probes_0 = probes_2;

#line 282
    (&kernelContext_7)->scene_color_0 = scene_color_2;

#line 446
    thread FullscreenOutput_0 output_1;


    float2 _S80 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 449
    (&output_1)->uv_2 = _S80;
    (&output_1)->position_2 = float4(_S80 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 450
    thread vertexMain_Result_0 _S81;

#line 450
    (&_S81)->position_1 = output_1.position_2;

#line 450
    (&_S81)->uv_1 = output_1.uv_2;

#line 450
    return _S81;
}

