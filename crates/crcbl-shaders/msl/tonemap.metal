#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 79 "shaders/tonemap.slang"
float3 tonemap_0(float3 color_0, float exposure_0)
{
    return saturate(color_0 * float3(exposure_0) );
}


#line 90 "core"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 90
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 45 "shaders/tonemap.slang"
struct TonemapParams_0
{
    float exposure_1;
};


#line 1084 "core"
struct KernelContext_0
{
    texture2d<float, access::sample> scene_0;
    sampler sceneSampler_0;
    TonemapParams_0 constant* params_0;
};


#line 99 "shaders/tonemap.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> scene_1 [[texture(0)]], sampler sceneSampler_1 [[sampler(0)]], TonemapParams_0 constant* params_1 [[buffer(0)]])
{

#line 99
    thread KernelContext_0 kernelContext_0;

#line 99
    (&kernelContext_0)->scene_0 = scene_1;

#line 99
    (&kernelContext_0)->sceneSampler_0 = sceneSampler_1;

#line 99
    (&kernelContext_0)->params_0 = params_1;

#line 99
    pixelOutput_0 _S2 = { float4(tonemap_0(((scene_1).sample((sceneSampler_1), (_S1.uv_0))).xyz, params_1->exposure_1), 1.0f) };

#line 112
    return _S2;
}


#line 112
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 72
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 473 "core"
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> scene_2 [[texture(0)]], sampler sceneSampler_2 [[sampler(0)]], TonemapParams_0 constant* params_2 [[buffer(0)]])
{

#line 473
    thread KernelContext_0 kernelContext_1;

#line 473
    (&kernelContext_1)->scene_0 = scene_2;

#line 473
    (&kernelContext_1)->sceneSampler_0 = sceneSampler_2;

#line 473
    (&kernelContext_1)->params_0 = params_2;

#line 87 "shaders/tonemap.slang"
    thread FullscreenOutput_0 output_1;

    float2 _S3 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 89
    (&output_1)->uv_2 = _S3;

#line 94
    (&output_1)->position_2 = float4(_S3 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 94
    thread vertexMain_Result_0 _S4;

#line 94
    (&_S4)->position_1 = output_1.position_2;

#line 94
    (&_S4)->uv_1 = output_1.uv_2;

#line 94
    return _S4;
}

