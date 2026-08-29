#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

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


#line 41 "shaders/smaa_blend.slang"
struct SmaaParams_0
{
    float2 inv_source_0;
    float2 source_size_0;
};


#line 1084 "core"
struct KernelContext_0
{
    SmaaParams_0 constant* params_0;
    texture2d<float, access::sample> blend_0;
    sampler sourceSampler_0;
    texture2d<float, access::sample> source_0;
};


#line 91 "shaders/smaa_blend.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], SmaaParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> blend_1 [[texture(1)]], sampler sourceSampler_1 [[sampler(0)]], texture2d<float, access::sample> source_1 [[texture(0)]])
{

#line 91
    thread KernelContext_0 kernelContext_0;

#line 91
    (&kernelContext_0)->params_0 = params_1;

#line 91
    (&kernelContext_0)->blend_0 = blend_1;

#line 91
    (&kernelContext_0)->sourceSampler_0 = sourceSampler_1;

#line 91
    (&kernelContext_0)->source_0 = source_1;


    float2 texel_0 = params_1->inv_source_0;
    float4 _S2 = float4(_S1.uv_0, _S1.uv_0);

#line 95
    float4 offset_0 = float4(1.0f, 0.0f, 0.0f, 1.0f) * float4(params_1->inv_source_0, params_1->inv_source_0) + _S2;

#line 100
    thread float4 a_0;
    a_0.x = ((blend_1).sample((sourceSampler_1), (offset_0.xy), level((0.0f)))).w;
    a_0.y = ((blend_1).sample((sourceSampler_1), (offset_0.zw), level((0.0f)))).y;
    float4 own_0 = ((blend_1).sample((sourceSampler_1), (_S1.uv_0), level((0.0f))));
    a_0.w = own_0.x;
    a_0.z = own_0.z;



    if((dot(a_0, float4(1.0f, 1.0f, 1.0f, 1.0f))) < 0.00000999999974738f)
    {

#line 109
        pixelOutput_0 _S3 = { float4((((&kernelContext_0)->source_0).sample(((&kernelContext_0)->sourceSampler_0), (_S1.uv_0), level((0.0f)))).xyz, 1.0f) };

        return _S3;
    }

#line 117
    float4 _S4 = float4(0.0f, a_0.y, 0.0f, a_0.w);
    float2 _S5 = float2(a_0.y, a_0.w);

#line 118
    float4 blending_offset_0;

#line 118
    float2 blending_weight_0;
    if((max(a_0.x, a_0.z)) > (max(a_0.y, a_0.w)))
    {

        float2 _S6 = float2(a_0.x, a_0.z);

#line 122
        blending_offset_0 = float4(a_0.x, 0.0f, a_0.z, 0.0f);

#line 122
        blending_weight_0 = _S6;

#line 119
    }
    else
    {

#line 119
        blending_offset_0 = _S4;

#line 119
        blending_weight_0 = _S5;

#line 119
    }

#line 124
    float2 blending_weight_1 = blending_weight_0 / float2(dot(blending_weight_0, float2(1.0f, 1.0f))) ;

#line 129
    float4 blending_coord_0 = blending_offset_0 * float4(texel_0, - texel_0) + _S2;

#line 129
    pixelOutput_0 _S7 = { float4(float3(blending_weight_1.x)  * (((&kernelContext_0)->source_0).sample(((&kernelContext_0)->sourceSampler_0), (blending_coord_0.xy), level((0.0f)))).xyz + float3(blending_weight_1.y)  * (((&kernelContext_0)->source_0).sample(((&kernelContext_0)->sourceSampler_0), (blending_coord_0.zw), level((0.0f)))).xyz, 1.0f) };


    return _S7;
}


#line 132
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 71
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 71
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], SmaaParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> blend_2 [[texture(1)]], sampler sourceSampler_2 [[sampler(0)]], texture2d<float, access::sample> source_2 [[texture(0)]])
{

#line 71
    thread KernelContext_0 kernelContext_1;

#line 71
    (&kernelContext_1)->params_0 = params_2;

#line 71
    (&kernelContext_1)->blend_0 = blend_2;

#line 71
    (&kernelContext_1)->sourceSampler_0 = sourceSampler_2;

#line 71
    (&kernelContext_1)->source_0 = source_2;

#line 84
    thread FullscreenOutput_0 output_1;
    float2 _S8 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 85
    (&output_1)->uv_2 = _S8;
    (&output_1)->position_2 = float4(_S8 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 86
    thread vertexMain_Result_0 _S9;

#line 86
    (&_S9)->position_1 = output_1.position_2;

#line 86
    (&_S9)->uv_1 = output_1.uv_2;

#line 86
    return _S9;
}

