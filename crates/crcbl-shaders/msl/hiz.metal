#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 77 "shaders/hiz.slang"
struct HizOutput_0
{
    float depth_0 [[depth(any)]];
};


#line 2580 "core.meta.slang"
struct KernelContext_0
{
    depth2d<float, access::sample> source_0;
};


#line 95 "shaders/hiz.slang"
[[fragment]] HizOutput_0 fragmentMain(float4 position_0 [[position]], depth2d<float, access::sample> source_1 [[texture(0)]])
{

#line 95
    thread KernelContext_0 kernelContext_0;

#line 95
    (&kernelContext_0)->source_0 = source_1;

    thread uint width_0;
    thread uint height_0;



    (*((&width_0)) = (source_1).get_width(0)),(*((&height_0)) = (source_1).get_height(0));
    int _S1 = int(width_0);

#line 103
    int _S2 = int(height_0);

    int2 base_0 = int2(position_0.xy) * int2(int(2)) ;

#line 110
    int2 _S3 = int2(int(0), int(0));

#line 110
    int2 _S4 = int2(int(1), int(1));

#line 110
    int2 _S5 = int2(_S1, _S2) - _S4;

#line 110
    int3 _S6 = int3(clamp(base_0, _S3, _S5), int(0));

    int3 _S7 = int3(clamp(base_0 + int2(int(1), int(0)), _S3, _S5), int(0));

    int3 _S8 = int3(clamp(base_0 + int2(int(0), int(1)), _S3, _S5), int(0));

    int3 _S9 = int3(clamp(base_0 + _S4, _S3, _S5), int(0));

#line 115
    float _S10 = max(max(max(((source_1).read(vec<uint,2>(((_S6)).xy), uint(((_S6)).z))), ((source_1).read(vec<uint,2>(((_S7)).xy), uint(((_S7)).z)))), ((source_1).read(vec<uint,2>(((_S8)).xy), uint(((_S8)).z)))), ((source_1).read(vec<uint,2>(((_S9)).xy), uint(((_S9)).z))));

#line 121
    bool odd_x_0 = (_S1 & int(1)) == int(1);
    bool odd_y_0 = (_S2 & int(1)) == int(1);

#line 122
    float nearest_0;
    if(odd_x_0)
    {

        int3 _S11 = int3(clamp(base_0 + int2(int(2), int(0)), _S3, _S5), int(0));

        int3 _S12 = int3(clamp(base_0 + int2(int(2), int(1)), _S3, _S5), int(0));

#line 128
        nearest_0 = max(max(_S10, (((&kernelContext_0)->source_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z)))), (((&kernelContext_0)->source_0).read(vec<uint,2>(((_S12)).xy), uint(((_S12)).z))));

#line 123
    }
    else
    {

#line 123
        nearest_0 = _S10;

#line 123
    }

#line 130
    if(odd_y_0)
    {

        int3 _S13 = int3(clamp(base_0 + int2(int(0), int(2)), _S3, _S5), int(0));

        int3 _S14 = int3(clamp(base_0 + int2(int(1), int(2)), _S3, _S5), int(0));

#line 135
        nearest_0 = max(max(nearest_0, (((&kernelContext_0)->source_0).read(vec<uint,2>(((_S13)).xy), uint(((_S13)).z)))), (((&kernelContext_0)->source_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z))));

#line 130
    }

#line 130
    bool _S15;

#line 137
    if(odd_x_0)
    {

#line 137
        _S15 = odd_y_0;

#line 137
    }
    else
    {

#line 137
        _S15 = false;

#line 137
    }

#line 137
    if(_S15)
    {

        int3 _S16 = int3(clamp(base_0 + int2(int(2), int(2)), _S3, _S5), int(0));

#line 140
        nearest_0 = max(nearest_0, (((&kernelContext_0)->source_0).read(vec<uint,2>(((_S16)).xy), uint(((_S16)).z))));

#line 137
    }

#line 143
    thread HizOutput_0 output_0;
    (&output_0)->depth_0 = nearest_0;
    return output_0;
}


#line 145
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
};


#line 65
struct FullscreenOutput_0
{
    float4 position_2;
};


#line 65
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> source_2 [[texture(0)]])
{

#line 85
    thread FullscreenOutput_0 output_1;

#line 90
    (&output_1)->position_2 = float4(float2(float((index_0 << 1U) & 2U), float(index_0 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 90
    thread vertexMain_Result_0 _S17;

#line 90
    (&_S17)->position_1 = output_1.position_2;

#line 90
    return _S17;
}

