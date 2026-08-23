#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 48 "shaders/bloom_composite.slang"
struct BloomParams_0
{
    float2 inv_source_0;
    float karis_0;
    float strength_0;
};


#line 1084 "core"
struct KernelContext_0
{
    texture2d<float, access::sample> scene_0;
    BloomParams_0 constant* params_0;
    texture2d<float, access::sample> source_0;
    sampler sourceSampler_0;
};


#line 96 "shaders/bloom_composite.slang"
float3 tap_0(float2 uv_0, float2 offset_0, KernelContext_0 thread* kernelContext_0)
{
    return ((kernelContext_0->source_0).sample((kernelContext_0->sourceSampler_0), (uv_0 + offset_0 * kernelContext_0->params_0->inv_source_0))).xyz;
}


#line 106
float3 tent_0(float2 uv_1, KernelContext_0 thread* kernelContext_1)
{

#line 106
    float3 _S1 = tap_0(uv_1, float2(-1.0f, 1.0f), kernelContext_1);

#line 106
    float3 _S2 = tap_0(uv_1, float2(0.0f, 1.0f), kernelContext_1);

#line 106
    float3 _S3 = float3(2.0f) ;


    float3 sum_0 = _S1 + _S2 * _S3;

#line 109
    float3 _S4 = tap_0(uv_1, float2(1.0f, 1.0f), kernelContext_1);
    float3 sum_1 = sum_0 + _S4;

#line 110
    float3 _S5 = tap_0(uv_1, float2(-1.0f, 0.0f), kernelContext_1);
    float3 sum_2 = sum_1 + _S5 * _S3;

#line 111
    float3 _S6 = tap_0(uv_1, float2(0.0f, 0.0f), kernelContext_1);
    float3 sum_3 = sum_2 + _S6 * float3(4.0f) ;

#line 112
    float3 _S7 = tap_0(uv_1, float2(1.0f, 0.0f), kernelContext_1);
    float3 sum_4 = sum_3 + _S7 * _S3;

#line 113
    float3 _S8 = tap_0(uv_1, float2(-1.0f, -1.0f), kernelContext_1);
    float3 sum_5 = sum_4 + _S8;

#line 114
    float3 _S9 = tap_0(uv_1, float2(0.0f, -1.0f), kernelContext_1);
    float3 sum_6 = sum_5 + _S9 * _S3;

#line 115
    float3 _S10 = tap_0(uv_1, float2(1.0f, -1.0f), kernelContext_1);

    return (sum_6 + _S10) * float3(0.0625f) ;
}


#line 117
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 117
struct pixelInput_0
{
    float2 uv_2 [[user(TEXCOORD)]];
};


#line 132
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S11 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> scene_1 [[texture(0)]], BloomParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> source_1 [[texture(1)]], sampler sourceSampler_1 [[sampler(0)]])
{

#line 132
    thread KernelContext_0 kernelContext_2;

#line 132
    (&kernelContext_2)->scene_0 = scene_1;

#line 132
    (&kernelContext_2)->params_0 = params_1;

#line 132
    (&kernelContext_2)->source_0 = source_1;

#line 132
    (&kernelContext_2)->sourceSampler_0 = sourceSampler_1;


    int3 _S12 = int3(int2(position_0.xy), int(0));

#line 135
    float3 color_0 = ((scene_1).read(vec<uint,2>(((_S12)).xy), uint(((_S12)).z))).xyz;

#line 135
    float3 _S13 = tent_0(_S11.uv_2, &kernelContext_2);

#line 135
    pixelOutput_0 _S14 = { float4(color_0 + _S13 * float3((&kernelContext_2)->params_0->strength_0) , 1.0f) };


    return _S14;
}


#line 138
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_3 [[user(TEXCOORD)]];
};


#line 89
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_4;
};


#line 89
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> scene_2 [[texture(0)]], BloomParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> source_2 [[texture(1)]], sampler sourceSampler_2 [[sampler(0)]])
{

#line 89
    thread KernelContext_0 kernelContext_3;

#line 89
    (&kernelContext_3)->scene_0 = scene_2;

#line 89
    (&kernelContext_3)->params_0 = params_2;

#line 89
    (&kernelContext_3)->source_0 = source_2;

#line 89
    (&kernelContext_3)->sourceSampler_0 = sourceSampler_2;

#line 123
    thread FullscreenOutput_0 output_1;


    float2 _S15 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 126
    (&output_1)->uv_4 = _S15;
    (&output_1)->position_2 = float4(_S15 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 127
    thread vertexMain_Result_0 _S16;

#line 127
    (&_S16)->position_1 = output_1.position_2;

#line 127
    (&_S16)->uv_3 = output_1.uv_4;

#line 127
    return _S16;
}

