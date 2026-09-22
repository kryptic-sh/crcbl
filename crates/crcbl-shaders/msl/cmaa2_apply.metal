#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 90 "core"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 2578 "core.meta.slang"
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 53 "shaders/cmaa2_apply.slang"
struct Cmaa2Params_0
{
    uint viewport_x_0;
    uint viewport_y_0;
};


#line 127
struct KernelContext_0
{
    Cmaa2Params_0 constant* params_0;
    texture2d<float, access::sample> source_0;
    uint device* accum_0;
};


#line 113
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], Cmaa2Params_0 constant* params_1 [[buffer(1)]], texture2d<float, access::sample> source_1 [[texture(0)]], uint device* accum_1 [[buffer(0)]])
{

#line 113
    thread KernelContext_0 kernelContext_0;

#line 113
    (&kernelContext_0)->params_0 = params_1;

#line 113
    (&kernelContext_0)->source_0 = source_1;

#line 113
    (&kernelContext_0)->accum_0 = accum_1;

#line 120
    uint2 texel_0 = min(uint2(position_0.xy), uint2(params_1->viewport_x_0 - 1U, params_1->viewport_y_0 - 1U));

#line 125
    int3 _S2 = int3(int2(texel_0), int(0));

#line 125
    float3 own_0 = ((source_1).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z))).xyz;

    uint _S3 = (texel_0.y * params_1->viewport_x_0 + texel_0.x) * 4U;

#line 127
    float weight_0 = float(accum_1[_S3 + 3U]) * 9.5367431640625e-07f;
    if(weight_0 <= 0.0f)
    {

#line 128
        pixelOutput_0 _S4 = { float4(own_0, 1.0f) };

        return _S4;
    }

#line 137
    float3 blended_0 = float3(float((&kernelContext_0)->accum_0[_S3]), float((&kernelContext_0)->accum_0[_S3 + 1U]), float((&kernelContext_0)->accum_0[_S3 + 2U])) * float3(9.5367431640625e-07f) ;

#line 145
    if(weight_0 >= 1.0f)
    {

#line 145
        pixelOutput_0 _S5 = { float4(blended_0 / float3(weight_0) , 1.0f) };

        return _S5;
    }

#line 147
    pixelOutput_0 _S6 = { float4(own_0 * float3((1.0f - weight_0))  + blended_0, 1.0f) };

    return _S6;
}


#line 149
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 85
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 473 "core"
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], Cmaa2Params_0 constant* params_2 [[buffer(1)]], texture2d<float, access::sample> source_2 [[texture(0)]], uint device* accum_2 [[buffer(0)]])
{

#line 473
    thread KernelContext_0 kernelContext_1;

#line 473
    (&kernelContext_1)->params_0 = params_2;

#line 473
    (&kernelContext_1)->source_0 = source_2;

#line 473
    (&kernelContext_1)->accum_0 = accum_2;

#line 104 "shaders/cmaa2_apply.slang"
    thread FullscreenOutput_0 output_1;


    float2 _S7 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 107
    (&output_1)->uv_2 = _S7;
    (&output_1)->position_2 = float4(_S7 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 108
    thread vertexMain_Result_0 _S8;

#line 108
    (&_S8)->position_1 = output_1.position_2;

#line 108
    (&_S8)->uv_1 = output_1.uv_2;

#line 108
    return _S8;
}

