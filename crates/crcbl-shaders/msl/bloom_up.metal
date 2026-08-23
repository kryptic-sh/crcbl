#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 43 "shaders/bloom_up.slang"
struct BloomParams_0
{
    float2 inv_source_0;
    float karis_0;
    float strength_0;
};


#line 1084 "core"
struct KernelContext_0
{
    BloomParams_0 constant* params_0;
    texture2d<float, access::sample> source_0;
    sampler sourceSampler_0;
};


#line 73 "shaders/bloom_up.slang"
float3 tap_0(float2 uv_0, float2 offset_0, KernelContext_0 thread* kernelContext_0)
{
    return ((kernelContext_0->source_0).sample((kernelContext_0->sourceSampler_0), (uv_0 + offset_0 * kernelContext_0->params_0->inv_source_0))).xyz;
}


#line 83
float3 tent_0(float2 uv_1, KernelContext_0 thread* kernelContext_1)
{

#line 83
    float3 _S1 = tap_0(uv_1, float2(-1.0f, 1.0f), kernelContext_1);

#line 83
    float3 _S2 = tap_0(uv_1, float2(0.0f, 1.0f), kernelContext_1);

#line 83
    float3 _S3 = float3(2.0f) ;


    float3 sum_0 = _S1 + _S2 * _S3;

#line 86
    float3 _S4 = tap_0(uv_1, float2(1.0f, 1.0f), kernelContext_1);
    float3 sum_1 = sum_0 + _S4;

#line 87
    float3 _S5 = tap_0(uv_1, float2(-1.0f, 0.0f), kernelContext_1);
    float3 sum_2 = sum_1 + _S5 * _S3;

#line 88
    float3 _S6 = tap_0(uv_1, float2(0.0f, 0.0f), kernelContext_1);
    float3 sum_3 = sum_2 + _S6 * float3(4.0f) ;

#line 89
    float3 _S7 = tap_0(uv_1, float2(1.0f, 0.0f), kernelContext_1);
    float3 sum_4 = sum_3 + _S7 * _S3;

#line 90
    float3 _S8 = tap_0(uv_1, float2(-1.0f, -1.0f), kernelContext_1);
    float3 sum_5 = sum_4 + _S8;

#line 91
    float3 _S9 = tap_0(uv_1, float2(0.0f, -1.0f), kernelContext_1);
    float3 sum_6 = sum_5 + _S9 * _S3;

#line 92
    float3 _S10 = tap_0(uv_1, float2(1.0f, -1.0f), kernelContext_1);

    return (sum_6 + _S10) * float3(0.0625f) ;
}


#line 94
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 94
struct pixelInput_0
{
    float2 uv_2 [[user(TEXCOORD)]];
};


#line 109
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S11 [[stage_in]], float4 position_0 [[position]], BloomParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> source_1 [[texture(0)]], sampler sourceSampler_1 [[sampler(0)]])
{

#line 109
    thread KernelContext_0 kernelContext_2;

#line 109
    (&kernelContext_2)->params_0 = params_1;

#line 109
    (&kernelContext_2)->source_0 = source_1;

#line 109
    (&kernelContext_2)->sourceSampler_0 = sourceSampler_1;

#line 109
    float3 _S12 = tent_0(_S11.uv_2, &kernelContext_2);

#line 109
    pixelOutput_0 _S13 = { float4(_S12, 0.0f) };

#line 115
    return _S13;
}


#line 115
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_3 [[user(TEXCOORD)]];
};


#line 66
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_4;
};


#line 66
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], BloomParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> source_2 [[texture(0)]], sampler sourceSampler_2 [[sampler(0)]])
{

#line 66
    thread KernelContext_0 kernelContext_3;

#line 66
    (&kernelContext_3)->params_0 = params_2;

#line 66
    (&kernelContext_3)->source_0 = source_2;

#line 66
    (&kernelContext_3)->sourceSampler_0 = sourceSampler_2;

#line 100
    thread FullscreenOutput_0 output_1;


    float2 _S14 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 103
    (&output_1)->uv_4 = _S14;
    (&output_1)->position_2 = float4(_S14 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 104
    thread vertexMain_Result_0 _S15;

#line 104
    (&_S15)->position_1 = output_1.position_2;

#line 104
    (&_S15)->uv_3 = output_1.uv_4;

#line 104
    return _S15;
}

