#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 124 "shaders/upscale.slang"
float4 catmull_rom_weights_0(float f_0)
{
    float f2_0 = f_0 * f_0;
    float f3_0 = f2_0 * f_0;



    float _S1 = 0.5f * f_0;

#line 128
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


#line 73 "shaders/upscale.slang"
struct UpscaleParams_0
{
    float2 source_extent_0;
    float2 inv_source_0;
};


#line 99
struct KernelContext_0
{
    UpscaleParams_0 constant* params_0;
    texture2d<float, access::sample> source_0;
    sampler sourceSampler_0;
};


#line 152
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], UpscaleParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> source_1 [[texture(0)]], sampler sourceSampler_1 [[sampler(0)]])
{

#line 152
    thread KernelContext_0 kernelContext_0;

#line 152
    (&kernelContext_0)->params_0 = params_1;

#line 152
    (&kernelContext_0)->source_0 = source_1;

#line 152
    (&kernelContext_0)->sourceSampler_0 = sourceSampler_1;

#line 152
    float2 _S3 = float2(0.5f) ;

#line 159
    float2 pos_0 = _S2.uv_0 * params_1->source_extent_0 - _S3;
    float2 base_0 = floor(pos_0);
    float2 f_1 = pos_0 - base_0;

    float4 _S4 = catmull_rom_weights_0(f_1.x);
    float4 _S5 = catmull_rom_weights_0(f_1.y);

    float3 _S6 = float3(0.0f, 0.0f, 0.0f);

#line 166
    int j_0 = int(0);

#line 166
    float3 sum_0 = _S6;
    for(;;)
    {

#line 167
        if(j_0 < int(4))
        {
        }
        else
        {

#line 167
            break;
        }

#line 167
        int i_0 = int(0);

        for(;;)
        {

#line 169
            if(i_0 < int(4))
            {
            }
            else
            {

#line 169
                break;
            }


            float3 sum_1 = sum_0 + (((&kernelContext_0)->source_0).sample(((&kernelContext_0)->sourceSampler_0), ((base_0 + float2(float(i_0) - 1.0f, float(j_0) - 1.0f) + _S3) * (&kernelContext_0)->params_0->inv_source_0), level((0.0f)))).xyz * float3((_S4[i_0] * _S5[j_0])) ;

#line 169
            i_0 = i_0 + int(1);

#line 169
            sum_0 = sum_1;

#line 169
        }

#line 167
        j_0 = j_0 + int(1);

#line 167
    }

#line 167
    pixelOutput_0 _S7 = { float4(saturate(sum_0), 1.0f) };

#line 179
    return _S7;
}


#line 179
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 104
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 104
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], UpscaleParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> source_2 [[texture(0)]], sampler sourceSampler_2 [[sampler(0)]])
{

#line 104
    thread KernelContext_0 kernelContext_1;

#line 104
    (&kernelContext_1)->params_0 = params_2;

#line 104
    (&kernelContext_1)->source_0 = source_2;

#line 104
    (&kernelContext_1)->sourceSampler_0 = sourceSampler_2;

#line 143
    thread FullscreenOutput_0 output_1;


    float2 _S8 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 146
    (&output_1)->uv_2 = _S8;
    (&output_1)->position_2 = float4(_S8 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 147
    thread vertexMain_Result_0 _S9;

#line 147
    (&_S9)->position_1 = output_1.position_2;

#line 147
    (&_S9)->uv_1 = output_1.uv_2;

#line 147
    return _S9;
}

