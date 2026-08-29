#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 124 "shaders/smaa_edges.slang"
float luma_of_0(float3 color_0)
{
    return sqrt(dot(color_0, float3(0.2125999927520752f, 0.71520000696182251f, 0.07220000028610229f)));
}


#line 78
struct SmaaParams_0
{
    float2 inv_source_0;
    float2 source_size_0;
};


#line 1084 "core"
struct KernelContext_0
{
    SmaaParams_0 constant* params_0;
    texture2d<float, access::sample> source_0;
    sampler sourceSampler_0;
};


#line 137 "shaders/smaa_edges.slang"
float luma_at_0(float2 uv_0, float2 offset_0, KernelContext_0 thread* kernelContext_0)
{
    return luma_of_0(((kernelContext_0->source_0).sample((kernelContext_0->sourceSampler_0), (uv_0 + offset_0 * kernelContext_0->params_0->inv_source_0), level((0.0f)))).xyz);
}


#line 139
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 139
struct pixelInput_0
{
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 154
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], SmaaParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> source_1 [[texture(0)]], sampler sourceSampler_1 [[sampler(0)]])
{

#line 154
    thread KernelContext_0 kernelContext_1;

#line 154
    (&kernelContext_1)->params_0 = params_1;

#line 154
    (&kernelContext_1)->source_0 = source_1;

#line 154
    (&kernelContext_1)->sourceSampler_0 = sourceSampler_1;

#line 154
    float _S2 = luma_at_0(_S1.uv_1, float2(0.0f, 0.0f), &kernelContext_1);

#line 154
    float _S3 = luma_at_0(_S1.uv_1, float2(-1.0f, 0.0f), &kernelContext_1);

#line 154
    float _S4 = luma_at_0(_S1.uv_1, float2(0.0f, -1.0f), &kernelContext_1);

#line 165
    float2 _S5 = float2(_S3, _S4);

#line 165
    float2 _S6 = float2(_S2) ;

#line 165
    float2 delta_0 = abs(_S6 - _S5);
    float2 edges_0 = step(float2(0.10000000149011612f, 0.10000000149011612f), delta_0);


    if((edges_0.x + edges_0.y) == 0.0f)
    {

#line 169
        pixelOutput_0 _S7 = { float4(0.0f, 0.0f, 0.0f, 1.0f) };

        return _S7;
    }

#line 171
    float _S8 = luma_at_0(_S1.uv_1, float2(1.0f, 0.0f), &kernelContext_1);

#line 171
    float _S9 = luma_at_0(_S1.uv_1, float2(0.0f, 1.0f), &kernelContext_1);

#line 181
    float2 max_delta_0 = max(delta_0, abs(_S6 - float2(_S8, _S9)));

#line 181
    float _S10 = luma_at_0(_S1.uv_1, float2(-2.0f, 0.0f), &kernelContext_1);

#line 181
    float _S11 = luma_at_0(_S1.uv_1, float2(0.0f, -2.0f), &kernelContext_1);

#line 186
    float2 max_delta_1 = max(max_delta_0, abs(_S5 - float2(_S10, _S11)));

    float _S12 = max(max_delta_1.x, max_delta_1.y);

#line 188
    pixelOutput_0 _S13 = { float4(edges_0 * step(float2(_S12, _S12), float2(2.0f)  * delta_0), 0.0f, 1.0f) };

#line 193
    return _S13;
}


#line 193
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_2 [[user(TEXCOORD)]];
};


#line 104
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_3;
};


#line 104
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], SmaaParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> source_2 [[texture(0)]], sampler sourceSampler_2 [[sampler(0)]])
{

#line 104
    thread KernelContext_0 kernelContext_2;

#line 104
    (&kernelContext_2)->params_0 = params_2;

#line 104
    (&kernelContext_2)->source_0 = source_2;

#line 104
    (&kernelContext_2)->sourceSampler_0 = sourceSampler_2;

#line 145
    thread FullscreenOutput_0 output_1;


    float2 _S14 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 148
    (&output_1)->uv_3 = _S14;
    (&output_1)->position_2 = float4(_S14 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 149
    thread vertexMain_Result_0 _S15;

#line 149
    (&_S15)->position_1 = output_1.position_2;

#line 149
    (&_S15)->uv_2 = output_1.uv_3;

#line 149
    return _S15;
}

