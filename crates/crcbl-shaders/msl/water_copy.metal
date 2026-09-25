#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 54 "shaders/water_copy.slang"
struct CopyOutput_0
{
    float4 color_0 [[color(0)]];
    float depth_0 [[depth(any)]];
};


#line 44
struct KernelContext_0
{
    texture2d<float, access::sample> source_color_0;
    depth2d<float, access::sample> source_depth_0;
};


#line 72
[[fragment]] CopyOutput_0 fragmentMain(float4 position_0 [[position]], texture2d<float, access::sample> source_color_1 [[texture(0)]], depth2d<float, access::sample> source_depth_1 [[texture(1)]])
{

#line 72
    thread KernelContext_0 kernelContext_0;

#line 72
    (&kernelContext_0)->source_color_0 = source_color_1;

#line 72
    (&kernelContext_0)->source_depth_0 = source_depth_1;

    int3 texel_0 = int3(int2(position_0.xy), int(0));
    thread CopyOutput_0 output_0;
    (&output_0)->color_0 = ((source_color_1).read(vec<uint,2>(((texel_0)).xy), uint(((texel_0)).z)));
    (&output_0)->depth_0 = ((source_depth_1).read(vec<uint,2>(((texel_0)).xy), uint(((texel_0)).z)));
    return output_0;
}


#line 78
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
};


#line 46
struct FullscreenOutput_0
{
    float4 position_2;
};


#line 484 "core"
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> source_color_2 [[texture(0)]], depth2d<float, access::sample> source_depth_2 [[texture(1)]])
{

#line 484
    thread KernelContext_0 kernelContext_1;

#line 484
    (&kernelContext_1)->source_color_0 = source_color_2;

#line 484
    (&kernelContext_1)->source_depth_0 = source_depth_2;

#line 63 "shaders/water_copy.slang"
    thread FullscreenOutput_0 output_1;



    (&output_1)->position_2 = float4(float2(float((index_0 << 1U) & 2U), float(index_0 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 67
    thread vertexMain_Result_0 _S1;

#line 67
    (&_S1)->position_1 = output_1.position_2;

#line 67
    return _S1;
}

