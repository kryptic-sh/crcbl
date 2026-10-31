#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 150 "shaders/upscale.slang"
float4 catmull_rom_weights_0(float f_0)
{
    float f2_0 = f_0 * f_0;
    float f3_0 = f2_0 * f_0;



    float _S1 = 0.5f * f_0;

#line 154
    return float4(-0.5f * f3_0 + f2_0 - _S1, 1.5f * f3_0 - 2.5f * f2_0 + 1.0f, -1.5f * f3_0 + 2.0f * f2_0 + _S1, 0.5f * f3_0 - 0.5f * f2_0);
}


#line 175
float merged_pair_offset_0(float4 w_0)
{
    float _S2 = w_0.z;

#line 177
    return _S2 / (w_0.y + _S2);
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


#line 92 "shaders/upscale.slang"
struct UpscaleParams_0
{
    float2 source_extent_0;
    float2 inv_source_0;
};


#line 125
struct KernelContext_0
{
    UpscaleParams_0 constant* params_0;
    texture2d<float, access::sample> source_0;
    sampler sourceSampler_0;
};


#line 192
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S3 [[stage_in]], float4 position_0 [[position]], UpscaleParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> source_1 [[texture(0)]], sampler sourceSampler_1 [[sampler(0)]])
{

#line 192
    thread KernelContext_0 kernelContext_0;

#line 192
    (&kernelContext_0)->params_0 = params_1;

#line 192
    (&kernelContext_0)->source_0 = source_1;

#line 192
    (&kernelContext_0)->sourceSampler_0 = sourceSampler_1;

#line 199
    float2 pos_0 = _S3.uv_0 * params_1->source_extent_0 - float2(0.5f) ;
    float2 base_0 = floor(pos_0);
    float2 f_1 = pos_0 - base_0;

    float4 wx_0 = catmull_rom_weights_0(f_1.x);
    float4 wy_0 = catmull_rom_weights_0(f_1.y);

#line 209
    float3 _S4 = float3(wx_0.x, wx_0.y + wx_0.z, wx_0.w);
    float3 _S5 = float3(wy_0.x, wy_0.y + wy_0.z, wy_0.w);

#line 210
    float3 _S6 = float3(0.5f) ;
    float3 _S7 = (float3(base_0.x)  + float3(-1.0f, merged_pair_offset_0(wx_0), 2.0f) + _S6) * float3(params_1->inv_source_0.x) ;
    float3 _S8 = (float3(base_0.y)  + float3(-1.0f, merged_pair_offset_0(wy_0), 2.0f) + _S6) * float3(params_1->inv_source_0.y) ;

    float3 _S9 = float3(0.0f, 0.0f, 0.0f);

#line 214
    int j_0 = int(0);

#line 214
    float3 sum_0 = _S9;
    for(;;)
    {

#line 215
        if(j_0 < int(3))
        {
        }
        else
        {

#line 215
            break;
        }

#line 215
        int i_0 = int(0);

        for(;;)
        {

#line 217
            if(i_0 < int(3))
            {
            }
            else
            {

#line 217
                break;
            }

            float3 sum_1 = sum_0 + (((&kernelContext_0)->source_0).sample(((&kernelContext_0)->sourceSampler_0), (float2(_S7[i_0], _S8[j_0])), level((0.0f)))).xyz * float3((_S4[i_0] * _S5[j_0])) ;

#line 217
            i_0 = i_0 + int(1);

#line 217
            sum_0 = sum_1;

#line 217
        }

#line 215
        j_0 = j_0 + int(1);

#line 215
    }

#line 215
    pixelOutput_0 _S10 = { float4(saturate(sum_0), 1.0f) };

#line 226
    return _S10;
}


#line 226
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 130
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 130
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], UpscaleParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> source_2 [[texture(0)]], sampler sourceSampler_2 [[sampler(0)]])
{

#line 130
    thread KernelContext_0 kernelContext_1;

#line 130
    (&kernelContext_1)->params_0 = params_2;

#line 130
    (&kernelContext_1)->source_0 = source_2;

#line 130
    (&kernelContext_1)->sourceSampler_0 = sourceSampler_2;

#line 183
    thread FullscreenOutput_0 output_1;


    float2 _S11 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 186
    (&output_1)->uv_2 = _S11;
    (&output_1)->position_2 = float4(_S11 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 187
    thread vertexMain_Result_0 _S12;

#line 187
    (&_S12)->position_1 = output_1.position_2;

#line 187
    (&_S12)->uv_1 = output_1.uv_2;

#line 187
    return _S12;
}

