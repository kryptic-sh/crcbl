#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 70 "shaders/bloom_down.slang"
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


#line 116 "shaders/bloom_down.slang"
float3 tap_0(float2 uv_0, float2 offset_0, KernelContext_0 thread* kernelContext_0)
{
    return ((kernelContext_0->source_0).sample((kernelContext_0->sourceSampler_0), (uv_0 + offset_0 * kernelContext_0->params_0->inv_source_0))).xyz;
}


#line 110
float luma_0(float3 color_0)
{
    return dot(color_0, float3(0.2125999927520752f, 0.71520000696182251f, 0.07220000028610229f));
}


#line 112
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 112
struct pixelInput_0
{
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 137
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], BloomParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> source_1 [[texture(0)]], sampler sourceSampler_1 [[sampler(0)]])
{

#line 137
    thread KernelContext_0 kernelContext_1;

#line 137
    (&kernelContext_1)->params_0 = params_1;

#line 137
    (&kernelContext_1)->source_0 = source_1;

#line 137
    (&kernelContext_1)->sourceSampler_0 = sourceSampler_1;

#line 137
    float3 _S2 = tap_0(_S1.uv_1, float2(-2.0f, 2.0f), &kernelContext_1);

#line 137
    float3 _S3 = tap_0(_S1.uv_1, float2(0.0f, 2.0f), &kernelContext_1);

#line 137
    float3 _S4 = tap_0(_S1.uv_1, float2(2.0f, 2.0f), &kernelContext_1);

#line 137
    float3 _S5 = tap_0(_S1.uv_1, float2(-2.0f, 0.0f), &kernelContext_1);

#line 137
    float3 _S6 = tap_0(_S1.uv_1, float2(0.0f, 0.0f), &kernelContext_1);

#line 137
    float3 _S7 = tap_0(_S1.uv_1, float2(2.0f, 0.0f), &kernelContext_1);

#line 137
    float3 _S8 = tap_0(_S1.uv_1, float2(-2.0f, -2.0f), &kernelContext_1);

#line 137
    float3 _S9 = tap_0(_S1.uv_1, float2(0.0f, -2.0f), &kernelContext_1);

#line 137
    float3 _S10 = tap_0(_S1.uv_1, float2(2.0f, -2.0f), &kernelContext_1);

#line 137
    float3 _S11 = tap_0(_S1.uv_1, float2(-1.0f, 1.0f), &kernelContext_1);

#line 137
    float3 _S12 = tap_0(_S1.uv_1, float2(1.0f, 1.0f), &kernelContext_1);

#line 137
    float3 _S13 = tap_0(_S1.uv_1, float2(-1.0f, -1.0f), &kernelContext_1);

#line 137
    float3 _S14 = tap_0(_S1.uv_1, float2(1.0f, -1.0f), &kernelContext_1);

#line 137
    float3 _S15 = float3(0.25f) ;

#line 158
    float3 g0_0 = (_S2 + _S3 + _S5 + _S6) * _S15;
    float3 g1_0 = (_S3 + _S4 + _S6 + _S7) * _S15;
    float3 g2_0 = (_S5 + _S6 + _S8 + _S9) * _S15;
    float3 g3_0 = (_S6 + _S7 + _S9 + _S10) * _S15;
    float3 g4_0 = (_S11 + _S12 + _S13 + _S14) * _S15;

#line 167
    float w0_0 = 0.125f / (1.0f + (&kernelContext_1)->params_0->karis_0 * luma_0(g0_0));
    float w1_0 = 0.125f / (1.0f + (&kernelContext_1)->params_0->karis_0 * luma_0(g1_0));
    float w2_0 = 0.125f / (1.0f + (&kernelContext_1)->params_0->karis_0 * luma_0(g2_0));
    float w3_0 = 0.125f / (1.0f + (&kernelContext_1)->params_0->karis_0 * luma_0(g3_0));
    float w4_0 = 0.5f / (1.0f + (&kernelContext_1)->params_0->karis_0 * luma_0(g4_0));

#line 171
    pixelOutput_0 _S16 = { float4((g0_0 * float3(w0_0)  + g1_0 * float3(w1_0)  + g2_0 * float3(w2_0)  + g3_0 * float3(w3_0)  + g4_0 * float3(w4_0) ) / float3((w0_0 + w1_0 + w2_0 + w3_0 + w4_0)) , 1.0f) };

#line 179
    return _S16;
}


#line 179
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_2 [[user(TEXCOORD)]];
};


#line 99
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_3;
};


#line 99
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], BloomParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> source_2 [[texture(0)]], sampler sourceSampler_2 [[sampler(0)]])
{

#line 99
    thread KernelContext_0 kernelContext_2;

#line 99
    (&kernelContext_2)->params_0 = params_2;

#line 99
    (&kernelContext_2)->source_0 = source_2;

#line 99
    (&kernelContext_2)->sourceSampler_0 = sourceSampler_2;

#line 124
    thread FullscreenOutput_0 output_1;



    float2 _S17 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 128
    (&output_1)->uv_3 = _S17;



    (&output_1)->position_2 = float4(_S17 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 132
    thread vertexMain_Result_0 _S18;

#line 132
    (&_S18)->position_1 = output_1.position_2;

#line 132
    (&_S18)->uv_2 = output_1.uv_3;

#line 132
    return _S18;
}

