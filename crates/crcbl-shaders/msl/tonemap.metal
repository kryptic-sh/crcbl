#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 114 "shaders/tonemap.slang"
float3 rrt_and_odt_fit_0(float3 v_0)
{


    return (v_0 * (v_0 + float3(0.02457859925925732f) ) - float3(0.0000905370034161f) ) / (v_0 * (float3(0.98372900485992432f)  * v_0 + float3(0.43295100331306458f) ) + float3(0.23808099329471588f) );
}


#line 146
float3 tonemap_0(float3 color_0, float exposure_0, uint curve_0)
{
    float3 exposed_0 = color_0 * float3(exposure_0) ;
    if(curve_0 == 1U)
    {

        return saturate((((rrt_and_odt_fit_0((((exposed_0) * (matrix<float,int(3),int(3)> (0.59719002246856689f, 0.35457998514175415f, 0.04822999984025955f, 0.07599999755620956f, 0.9083399772644043f, 0.01565999910235405f, 0.0284000001847744f, 0.1338299959897995f, 0.83776998519897461f)))))) * (matrix<float,int(3),int(3)> (1.60475003719329834f, -0.53108000755310059f, -0.07366999983787537f, -0.10208000242710114f, 1.10812997817993164f, -0.00604999996721745f, -0.00326999998651445f, -0.07276000082492828f, 1.0760200023651123f)))));
    }
    return saturate(exposed_0);
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


#line 50 "shaders/tonemap.slang"
struct TonemapParams_0
{
    float exposure_1;
    uint curve_1;
};


#line 1084 "core"
struct KernelContext_0
{
    texture2d<float, access::sample> scene_0;
    sampler sceneSampler_0;
    TonemapParams_0 constant* params_0;
};


#line 172 "shaders/tonemap.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> scene_1 [[texture(0)]], sampler sceneSampler_1 [[sampler(0)]], TonemapParams_0 constant* params_1 [[buffer(0)]])
{

#line 172
    thread KernelContext_0 kernelContext_0;

#line 172
    (&kernelContext_0)->scene_0 = scene_1;

#line 172
    (&kernelContext_0)->sceneSampler_0 = sceneSampler_1;

#line 172
    (&kernelContext_0)->params_0 = params_1;

#line 172
    pixelOutput_0 _S2 = { float4(tonemap_0(((scene_1).sample((sceneSampler_1), (_S1.uv_0))).xyz, params_1->exposure_1, params_1->curve_1), 1.0f) };

#line 185
    return _S2;
}


#line 185
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 134
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 473 "core"
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> scene_2 [[texture(0)]], sampler sceneSampler_2 [[sampler(0)]], TonemapParams_0 constant* params_2 [[buffer(0)]])
{

#line 473
    thread KernelContext_0 kernelContext_1;

#line 473
    (&kernelContext_1)->scene_0 = scene_2;

#line 473
    (&kernelContext_1)->sceneSampler_0 = sceneSampler_2;

#line 473
    (&kernelContext_1)->params_0 = params_2;

#line 160 "shaders/tonemap.slang"
    thread FullscreenOutput_0 output_1;

    float2 _S3 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 162
    (&output_1)->uv_2 = _S3;

#line 167
    (&output_1)->position_2 = float4(_S3 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 167
    thread vertexMain_Result_0 _S4;

#line 167
    (&_S4)->position_1 = output_1.position_2;

#line 167
    (&_S4)->uv_1 = output_1.uv_2;

#line 167
    return _S4;
}

