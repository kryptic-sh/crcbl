#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 112 "shaders/upscale.slang"
float4 catmull_rom_weights_0(float f_0)
{
    float f2_0 = f_0 * f_0;
    float f3_0 = f2_0 * f_0;



    float _S1 = 0.5f * f_0;

#line 116
    return float4(-0.5f * f3_0 + f2_0 - _S1, 1.5f * f3_0 - 2.5f * f2_0 + 1.0f, -1.5f * f3_0 + 2.0f * f2_0 + _S1, 0.5f * f3_0 - 0.5f * f2_0);
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


#line 61 "shaders/upscale.slang"
struct UpscaleParams_0
{
    float2 source_extent_0;
    float2 inv_source_0;
};


#line 1084 "core"
struct KernelContext_0
{
    UpscaleParams_0 constant* params_0;
    texture2d<float, access::sample> source_0;
    sampler sourceSampler_0;
};


#line 140 "shaders/upscale.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], UpscaleParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> source_1 [[texture(0)]], sampler sourceSampler_1 [[sampler(0)]])
{

#line 140
    thread KernelContext_0 kernelContext_0;

#line 140
    (&kernelContext_0)->params_0 = params_1;

#line 140
    (&kernelContext_0)->source_0 = source_1;

#line 140
    (&kernelContext_0)->sourceSampler_0 = sourceSampler_1;

#line 140
    float2 _S3 = float2(0.5f) ;

#line 147
    float2 pos_0 = _S2.uv_0 * params_1->source_extent_0 - _S3;
    float2 base_0 = floor(pos_0);
    float2 f_1 = pos_0 - base_0;

    float4 _S4 = catmull_rom_weights_0(f_1.x);
    float4 _S5 = catmull_rom_weights_0(f_1.y);

    float3 _S6 = float3(0.0f, 0.0f, 0.0f);

#line 154
    int j_0 = int(0);

#line 154
    float3 sum_0 = _S6;
    for(;;)
    {

#line 155
        if(j_0 < int(4))
        {
        }
        else
        {

#line 155
            break;
        }

#line 155
        int i_0 = int(0);

        for(;;)
        {

#line 157
            if(i_0 < int(4))
            {
            }
            else
            {

#line 157
                break;
            }


            float3 sum_1 = sum_0 + (((&kernelContext_0)->source_0).sample(((&kernelContext_0)->sourceSampler_0), ((base_0 + float2(float(i_0) - 1.0f, float(j_0) - 1.0f) + _S3) * (&kernelContext_0)->params_0->inv_source_0), level((0.0f)))).xyz * float3((_S4[i_0] * _S5[j_0])) ;

#line 157
            i_0 = i_0 + int(1);

#line 157
            sum_0 = sum_1;

#line 157
        }

#line 155
        j_0 = j_0 + int(1);

#line 155
    }

#line 155
    pixelOutput_0 _S7 = { float4(saturate(sum_0), 1.0f) };

#line 167
    return _S7;
}


#line 167
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 92
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 92
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], UpscaleParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> source_2 [[texture(0)]], sampler sourceSampler_2 [[sampler(0)]])
{

#line 92
    thread KernelContext_0 kernelContext_1;

#line 92
    (&kernelContext_1)->params_0 = params_2;

#line 92
    (&kernelContext_1)->source_0 = source_2;

#line 92
    (&kernelContext_1)->sourceSampler_0 = sourceSampler_2;

#line 131
    thread FullscreenOutput_0 output_1;


    float2 _S8 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 134
    (&output_1)->uv_2 = _S8;
    (&output_1)->position_2 = float4(_S8 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 135
    thread vertexMain_Result_0 _S9;

#line 135
    (&_S9)->position_1 = output_1.position_2;

#line 135
    (&_S9)->uv_1 = output_1.uv_2;

#line 135
    return _S9;
}

