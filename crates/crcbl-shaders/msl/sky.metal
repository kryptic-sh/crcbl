#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 58 "shaders/sky.slang"
struct SkyParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    array<float4, int(3)> sky_0;
    float4 atmosphere_0;
};


#line 58
struct KernelContext_0
{
    SkyParams_natural_0 constant* camera_0;
    packed_float4 device* sky_view_0;
};


#line 155
float3 sky_view_at_0(float up_0, float azimuth_cosine_0, KernelContext_0 thread* kernelContext_0)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_0 = clamp(up_0, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_0));

#line 159
    float _S1;
    if(clamped_0 >= 0.0f)
    {

#line 160
        _S1 = root_0;

#line 160
    }
    else
    {

#line 160
        _S1 = - root_0;

#line 160
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    float x0_0 = clamp(floor(across_0), 0.0f, 95.0f);

    float fx_0 = clamp(across_0 - x0_0, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S1, 0.0f, 1.0f) * 64.0f - 0.5f;
    float y0_0 = clamp(floor(down_0), 0.0f, 63.0f);

    float fy_0 = clamp(down_0 - y0_0, 0.0f, 1.0f);

    uint row0_0 = uint(y0_0) * 96U;
    uint row1_0 = uint(min(y0_0 + 1.0f, 63.0f)) * 96U;
    uint _S2 = uint(x0_0);

#line 174
    float3 _S3 = float3((1.0f - fx_0)) ;
    uint _S4 = uint(min(x0_0 + 1.0f, 95.0f));

#line 175
    float3 _S5 = float3(fx_0) ;


    return ((float4(*(kernelContext_0->sky_view_0+(row0_0 + _S2))) ).xyz * _S3 + (float4(*(kernelContext_0->sky_view_0+(row0_0 + _S4))) ).xyz * _S5) * float3((1.0f - fy_0))  + ((float4(*(kernelContext_0->sky_view_0+(row1_0 + _S2))) ).xyz * _S3 + (float4(*(kernelContext_0->sky_view_0+(row1_0 + _S4))) ).xyz * _S5) * float3(fy_0) ;
}


#line 188
float3 atmosphere_radiance_0(float3 direction_0, KernelContext_0 thread* kernelContext_1)
{
    float3 sun_0 = kernelContext_1->camera_0->atmosphere_0.xyz;
    float _S6 = direction_0.x;

#line 191
    float _S7 = direction_0.z;

#line 191
    float view_flat_0 = sqrt(_S6 * _S6 + _S7 * _S7);
    float _S8 = sun_0.x;

#line 192
    float _S9 = sun_0.z;

#line 192
    float sun_flat_0 = sqrt(_S8 * _S8 + _S9 * _S9);

#line 192
    bool _S10;

    if(view_flat_0 > 0.0f)
    {

#line 194
        _S10 = sun_flat_0 > 0.0f;

#line 194
    }
    else
    {

#line 194
        _S10 = false;

#line 194
    }

#line 194
    float cosine_0;

#line 194
    if(_S10)
    {

#line 194
        cosine_0 = (_S6 * _S8 + _S7 * _S9) / (view_flat_0 * sun_flat_0);

#line 194
    }
    else
    {

#line 194
        cosine_0 = 1.0f;

#line 194
    }

#line 194
    float3 _S11 = sky_view_at_0(direction_0.y, cosine_0, kernelContext_1);



    return _S11;
}


#line 138
float3 sky_radiance_0(float3 direction_1, KernelContext_0 thread* kernelContext_2)
{
    float up_1 = clamp(direction_1.y, -1.0f, 1.0f);

#line 140
    float3 far_0;
    if(up_1 >= 0.0f)
    {

#line 141
        far_0 = kernelContext_2->camera_0->sky_0[int(0)].xyz;

#line 141
    }
    else
    {

#line 141
        far_0 = kernelContext_2->camera_0->sky_0[int(2)].xyz;

#line 141
    }
    float u_1 = abs(up_1);
    float blend_0 = u_1 * u_1 * (3.0f - 2.0f * u_1);
    return kernelContext_2->camera_0->sky_0[int(1)].xyz * float3((1.0f - blend_0))  + far_0 * float3(blend_0) ;
}


#line 144
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 128
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 215
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S12 [[stage_in]], float4 position_0 [[position]], SkyParams_natural_0 constant* camera_1 [[buffer(0)]], packed_float4 device* sky_view_1 [[buffer(1)]])
{

#line 215
    thread KernelContext_0 kernelContext_3;

#line 215
    (&kernelContext_3)->camera_0 = camera_1;

#line 215
    (&kernelContext_3)->sky_view_0 = sky_view_1;

#line 223
    float2 ndc_0 = float2(_S12.uv_0.x * 2.0f - 1.0f, 1.0f - _S12.uv_0.y * 2.0f);

    float4 near_plane_0 = (((float4(ndc_0, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> (camera_1->inv_proj_0.data_0[int(0)][int(0)], camera_1->inv_proj_0.data_0[int(1)][int(0)], camera_1->inv_proj_0.data_0[int(2)][int(0)], camera_1->inv_proj_0.data_0[int(3)][int(0)], camera_1->inv_proj_0.data_0[int(0)][int(1)], camera_1->inv_proj_0.data_0[int(1)][int(1)], camera_1->inv_proj_0.data_0[int(2)][int(1)], camera_1->inv_proj_0.data_0[int(3)][int(1)], camera_1->inv_proj_0.data_0[int(0)][int(2)], camera_1->inv_proj_0.data_0[int(1)][int(2)], camera_1->inv_proj_0.data_0[int(2)][int(2)], camera_1->inv_proj_0.data_0[int(3)][int(2)], camera_1->inv_proj_0.data_0[int(0)][int(3)], camera_1->inv_proj_0.data_0[int(1)][int(3)], camera_1->inv_proj_0.data_0[int(2)][int(3)], camera_1->inv_proj_0.data_0[int(3)][int(3)]))));
    float4 beyond_0 = (((float4(ndc_0, 0.5f, 1.0f)) * (matrix<float,int(4),int(4)> (camera_1->inv_proj_0.data_0[int(0)][int(0)], camera_1->inv_proj_0.data_0[int(1)][int(0)], camera_1->inv_proj_0.data_0[int(2)][int(0)], camera_1->inv_proj_0.data_0[int(3)][int(0)], camera_1->inv_proj_0.data_0[int(0)][int(1)], camera_1->inv_proj_0.data_0[int(1)][int(1)], camera_1->inv_proj_0.data_0[int(2)][int(1)], camera_1->inv_proj_0.data_0[int(3)][int(1)], camera_1->inv_proj_0.data_0[int(0)][int(2)], camera_1->inv_proj_0.data_0[int(1)][int(2)], camera_1->inv_proj_0.data_0[int(2)][int(2)], camera_1->inv_proj_0.data_0[int(3)][int(2)], camera_1->inv_proj_0.data_0[int(0)][int(3)], camera_1->inv_proj_0.data_0[int(1)][int(3)], camera_1->inv_proj_0.data_0[int(2)][int(3)], camera_1->inv_proj_0.data_0[int(3)][int(3)]))));

    float3 direction_2 = normalize((((float4(beyond_0.xyz / float3(beyond_0.w)  - near_plane_0.xyz / float3(near_plane_0.w) , 0.0f)) * (matrix<float,int(4),int(4)> (camera_1->inv_view_0.data_0[int(0)][int(0)], camera_1->inv_view_0.data_0[int(1)][int(0)], camera_1->inv_view_0.data_0[int(2)][int(0)], camera_1->inv_view_0.data_0[int(3)][int(0)], camera_1->inv_view_0.data_0[int(0)][int(1)], camera_1->inv_view_0.data_0[int(1)][int(1)], camera_1->inv_view_0.data_0[int(2)][int(1)], camera_1->inv_view_0.data_0[int(3)][int(1)], camera_1->inv_view_0.data_0[int(0)][int(2)], camera_1->inv_view_0.data_0[int(1)][int(2)], camera_1->inv_view_0.data_0[int(2)][int(2)], camera_1->inv_view_0.data_0[int(3)][int(2)], camera_1->inv_view_0.data_0[int(0)][int(3)], camera_1->inv_view_0.data_0[int(1)][int(3)], camera_1->inv_view_0.data_0[int(2)][int(3)], camera_1->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 228
    float3 radiance_0;


    if((camera_1->atmosphere_0.w) > 0.0f)
    {

#line 231
        float3 _S13 = atmosphere_radiance_0(direction_2, &kernelContext_3);

#line 231
        radiance_0 = _S13;

#line 231
    }
    else
    {

#line 231
        float3 _S14 = sky_radiance_0(direction_2, &kernelContext_3);

#line 231
        radiance_0 = _S14;

#line 231
    }

#line 231
    pixelOutput_0 _S15 = { float4(radiance_0, 1.0f) };

    return _S15;
}


#line 233
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 128
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 473 "core"
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], SkyParams_natural_0 constant* camera_2 [[buffer(0)]], packed_float4 device* sky_view_2 [[buffer(1)]])
{

#line 473
    thread KernelContext_0 kernelContext_4;

#line 473
    (&kernelContext_4)->camera_0 = camera_2;

#line 473
    (&kernelContext_4)->sky_view_0 = sky_view_2;

#line 204 "shaders/sky.slang"
    thread FullscreenOutput_0 output_1;

#line 209
    float2 _S16 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 209
    (&output_1)->uv_2 = _S16;
    (&output_1)->position_2 = float4(_S16 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 210
    thread vertexMain_Result_0 _S17;

#line 210
    (&_S17)->position_1 = output_1.position_2;

#line 210
    (&_S17)->uv_1 = output_1.uv_2;

#line 210
    return _S17;
}

