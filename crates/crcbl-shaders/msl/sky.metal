#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 47 "shaders/sky.slang"
struct SkyParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    array<float4, int(3)> sky_0;
};


#line 1084 "core"
struct KernelContext_0
{
    SkyParams_natural_0 constant* camera_0;
};


#line 90 "shaders/sky.slang"
float3 sky_radiance_0(float3 direction_0, KernelContext_0 thread* kernelContext_0)
{
    float up_0 = clamp(direction_0.y, -1.0f, 1.0f);

#line 92
    float3 far_0;
    if(up_0 >= 0.0f)
    {

#line 93
        far_0 = kernelContext_0->camera_0->sky_0[int(0)].xyz;

#line 93
    }
    else
    {

#line 93
        far_0 = kernelContext_0->camera_0->sky_0[int(2)].xyz;

#line 93
    }
    float u_0 = abs(up_0);
    float blend_0 = u_0 * u_0 * (3.0f - 2.0f * u_0);
    return kernelContext_0->camera_0->sky_0[int(1)].xyz * float3((1.0f - blend_0))  + far_0 * float3(blend_0) ;
}


#line 96
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 80
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 113
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], SkyParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 113
    thread KernelContext_0 kernelContext_1;

#line 113
    (&kernelContext_1)->camera_0 = camera_1;

#line 121
    float2 ndc_0 = float2(_S1.uv_0.x * 2.0f - 1.0f, 1.0f - _S1.uv_0.y * 2.0f);

    float4 near_plane_0 = (((float4(ndc_0, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> (camera_1->inv_proj_0.data_0[int(0)][int(0)], camera_1->inv_proj_0.data_0[int(1)][int(0)], camera_1->inv_proj_0.data_0[int(2)][int(0)], camera_1->inv_proj_0.data_0[int(3)][int(0)], camera_1->inv_proj_0.data_0[int(0)][int(1)], camera_1->inv_proj_0.data_0[int(1)][int(1)], camera_1->inv_proj_0.data_0[int(2)][int(1)], camera_1->inv_proj_0.data_0[int(3)][int(1)], camera_1->inv_proj_0.data_0[int(0)][int(2)], camera_1->inv_proj_0.data_0[int(1)][int(2)], camera_1->inv_proj_0.data_0[int(2)][int(2)], camera_1->inv_proj_0.data_0[int(3)][int(2)], camera_1->inv_proj_0.data_0[int(0)][int(3)], camera_1->inv_proj_0.data_0[int(1)][int(3)], camera_1->inv_proj_0.data_0[int(2)][int(3)], camera_1->inv_proj_0.data_0[int(3)][int(3)]))));
    float4 beyond_0 = (((float4(ndc_0, 0.5f, 1.0f)) * (matrix<float,int(4),int(4)> (camera_1->inv_proj_0.data_0[int(0)][int(0)], camera_1->inv_proj_0.data_0[int(1)][int(0)], camera_1->inv_proj_0.data_0[int(2)][int(0)], camera_1->inv_proj_0.data_0[int(3)][int(0)], camera_1->inv_proj_0.data_0[int(0)][int(1)], camera_1->inv_proj_0.data_0[int(1)][int(1)], camera_1->inv_proj_0.data_0[int(2)][int(1)], camera_1->inv_proj_0.data_0[int(3)][int(1)], camera_1->inv_proj_0.data_0[int(0)][int(2)], camera_1->inv_proj_0.data_0[int(1)][int(2)], camera_1->inv_proj_0.data_0[int(2)][int(2)], camera_1->inv_proj_0.data_0[int(3)][int(2)], camera_1->inv_proj_0.data_0[int(0)][int(3)], camera_1->inv_proj_0.data_0[int(1)][int(3)], camera_1->inv_proj_0.data_0[int(2)][int(3)], camera_1->inv_proj_0.data_0[int(3)][int(3)]))));

#line 124
    float3 _S2 = sky_radiance_0(normalize((((float4(beyond_0.xyz / float3(beyond_0.w)  - near_plane_0.xyz / float3(near_plane_0.w) , 0.0f)) * (matrix<float,int(4),int(4)> (camera_1->inv_view_0.data_0[int(0)][int(0)], camera_1->inv_view_0.data_0[int(1)][int(0)], camera_1->inv_view_0.data_0[int(2)][int(0)], camera_1->inv_view_0.data_0[int(3)][int(0)], camera_1->inv_view_0.data_0[int(0)][int(1)], camera_1->inv_view_0.data_0[int(1)][int(1)], camera_1->inv_view_0.data_0[int(2)][int(1)], camera_1->inv_view_0.data_0[int(3)][int(1)], camera_1->inv_view_0.data_0[int(0)][int(2)], camera_1->inv_view_0.data_0[int(1)][int(2)], camera_1->inv_view_0.data_0[int(2)][int(2)], camera_1->inv_view_0.data_0[int(3)][int(2)], camera_1->inv_view_0.data_0[int(0)][int(3)], camera_1->inv_view_0.data_0[int(1)][int(3)], camera_1->inv_view_0.data_0[int(2)][int(3)], camera_1->inv_view_0.data_0[int(3)][int(3)])))).xyz), &kernelContext_1);

#line 124
    pixelOutput_0 _S3 = { float4(_S2, 1.0f) };

#line 129
    return _S3;
}


#line 129
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 80
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 473 "core"
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], SkyParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 102 "shaders/sky.slang"
    thread FullscreenOutput_0 output_1;

#line 107
    float2 _S4 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 107
    (&output_1)->uv_2 = _S4;
    (&output_1)->position_2 = float4(_S4 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 108
    thread vertexMain_Result_0 _S5;

#line 108
    (&_S5)->position_1 = output_1.position_2;

#line 108
    (&_S5)->uv_1 = output_1.uv_2;

#line 108
    return _S5;
}

