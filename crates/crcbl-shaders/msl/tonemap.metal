#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 54 "shaders/tonemap.slang"
float3 tonemap_0(float3 color_0)
{
    return saturate(color_0);
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


#line 90
struct KernelContext_0
{
    texture2d<float, access::sample> scene_0;
    sampler sceneSampler_0;
};


#line 74 "shaders/tonemap.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> scene_1 [[texture(0)]], sampler sceneSampler_1 [[sampler(0)]])
{

#line 74
    thread KernelContext_0 kernelContext_0;

#line 74
    (&kernelContext_0)->scene_0 = scene_1;

#line 74
    (&kernelContext_0)->sceneSampler_0 = sceneSampler_1;

#line 74
    pixelOutput_0 _S2 = { float4(tonemap_0(((scene_1).sample((sceneSampler_1), (_S1.uv_0))).xyz), 1.0f) };

#line 87
    return _S2;
}


#line 87
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 40
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 473 "core"
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> scene_2 [[texture(0)]], sampler sceneSampler_2 [[sampler(0)]])
{

#line 473
    thread KernelContext_0 kernelContext_1;

#line 473
    (&kernelContext_1)->scene_0 = scene_2;

#line 473
    (&kernelContext_1)->sceneSampler_0 = sceneSampler_2;

#line 62 "shaders/tonemap.slang"
    thread FullscreenOutput_0 output_1;

    float2 _S3 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 64
    (&output_1)->uv_2 = _S3;

#line 69
    (&output_1)->position_2 = float4(_S3 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 69
    thread vertexMain_Result_0 _S4;

#line 69
    (&_S4)->position_1 = output_1.position_2;

#line 69
    (&_S4)->uv_1 = output_1.uv_2;

#line 69
    return _S4;
}

