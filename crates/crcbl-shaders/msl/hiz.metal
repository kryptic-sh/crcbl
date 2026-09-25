#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 109 "shaders/hiz.slang"
float combine_0(float a_0, float b_0, bool farthest_0)
{

#line 109
    float _S1;

    if(farthest_0)
    {

#line 111
        _S1 = min(a_0, b_0);

#line 111
    }
    else
    {

#line 111
        _S1 = max(a_0, b_0);

#line 111
    }

#line 111
    return _S1;
}


#line 77
struct FullscreenOutput_0
{
    float4 position_0;
};


#line 75
struct KernelContext_0
{
    depth2d<float, access::sample> source_0;
};


#line 116
float reduce_0(const FullscreenOutput_0 thread* input_0, bool farthest_1, KernelContext_0 thread* kernelContext_0)
{

#line 116
    depth2d<float, access::sample> _S2 = kernelContext_0->source_0;

    thread uint width_0;
    thread uint height_0;



    (*((&width_0)) = (_S2).get_width(0)),(*((&height_0)) = (_S2).get_height(0));
    int _S3 = int(width_0);

#line 124
    int _S4 = int(height_0);

    int2 base_0 = int2(input_0->position_0.xy) * int2(int(2)) ;

#line 131
    int2 _S5 = int2(int(0), int(0));

#line 131
    int2 _S6 = int2(int(1), int(1));

#line 131
    int2 _S7 = int2(_S3, _S4) - _S6;

#line 131
    int3 _S8 = int3(clamp(base_0, _S5, _S7), int(0));

    int3 _S9 = int3(clamp(base_0 + int2(int(1), int(0)), _S5, _S7), int(0));

    int3 _S10 = int3(clamp(base_0 + int2(int(0), int(1)), _S5, _S7), int(0));

    int3 _S11 = int3(clamp(base_0 + _S6, _S5, _S7), int(0));

#line 136
    float value_0 = combine_0(combine_0(combine_0(((kernelContext_0->source_0).read(vec<uint,2>(((_S8)).xy), uint(((_S8)).z))), ((kernelContext_0->source_0).read(vec<uint,2>(((_S9)).xy), uint(((_S9)).z))), farthest_1), ((kernelContext_0->source_0).read(vec<uint,2>(((_S10)).xy), uint(((_S10)).z))), farthest_1), ((kernelContext_0->source_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z))), farthest_1);

#line 143
    bool odd_x_0 = (_S3 & int(1)) == int(1);
    bool odd_y_0 = (_S4 & int(1)) == int(1);

#line 144
    float value_1;
    if(odd_x_0)
    {

        int3 _S12 = int3(clamp(base_0 + int2(int(2), int(0)), _S5, _S7), int(0));

        int3 _S13 = int3(clamp(base_0 + int2(int(2), int(1)), _S5, _S7), int(0));

#line 150
        value_1 = combine_0(combine_0(value_0, ((kernelContext_0->source_0).read(vec<uint,2>(((_S12)).xy), uint(((_S12)).z))), farthest_1), ((kernelContext_0->source_0).read(vec<uint,2>(((_S13)).xy), uint(((_S13)).z))), farthest_1);

#line 145
    }
    else
    {

#line 145
        value_1 = value_0;

#line 145
    }

#line 152
    if(odd_y_0)
    {

        int3 _S14 = int3(clamp(base_0 + int2(int(0), int(2)), _S5, _S7), int(0));

        int3 _S15 = int3(clamp(base_0 + int2(int(1), int(2)), _S5, _S7), int(0));

#line 157
        value_1 = combine_0(combine_0(value_1, ((kernelContext_0->source_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z))), farthest_1), ((kernelContext_0->source_0).read(vec<uint,2>(((_S15)).xy), uint(((_S15)).z))), farthest_1);

#line 152
    }

#line 152
    bool _S16;

#line 159
    if(odd_x_0)
    {

#line 159
        _S16 = odd_y_0;

#line 159
    }
    else
    {

#line 159
        _S16 = false;

#line 159
    }

#line 159
    if(_S16)
    {

        int3 _S17 = int3(clamp(base_0 + int2(int(2), int(2)), _S5, _S7), int(0));

#line 162
        value_1 = combine_0(value_1, ((kernelContext_0->source_0).read(vec<uint,2>(((_S17)).xy), uint(((_S17)).z))), farthest_1);

#line 159
    }

#line 164
    return value_1;
}


#line 89
struct HizOutput_0
{
    float depth_0 [[depth(any)]];
};


#line 169
[[fragment]] HizOutput_0 fragmentMain(float4 position_1 [[position]], depth2d<float, access::sample> source_1 [[texture(0)]])
{

#line 169
    thread KernelContext_0 kernelContext_1;

#line 169
    (&kernelContext_1)->source_0 = source_1;

    thread HizOutput_0 output_0;

#line 171
    thread FullscreenOutput_0 _S18;

#line 171
    (&_S18)->position_0 = position_1;

#line 171
    float _S19 = reduce_0(&_S18, false, &kernelContext_1);
    (&output_0)->depth_0 = _S19;
    return output_0;
}


#line 191
[[fragment]] HizOutput_0 farthestMain(float4 position_2 [[position]], depth2d<float, access::sample> source_2 [[texture(0)]])
{

#line 191
    thread KernelContext_0 kernelContext_2;

#line 191
    (&kernelContext_2)->source_0 = source_2;

    thread HizOutput_0 output_1;

#line 193
    thread FullscreenOutput_0 _S20;

#line 193
    (&_S20)->position_0 = position_2;

#line 193
    float _S21 = reduce_0(&_S20, true, &kernelContext_2);
    (&output_1)->depth_0 = _S21;
    return output_1;
}


#line 195
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
};


#line 195
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> source_3 [[texture(0)]])
{

#line 97
    thread FullscreenOutput_0 output_2;

#line 102
    (&output_2)->position_0 = float4(float2(float((index_0 << 1U) & 2U), float(index_0 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 102
    thread vertexMain_Result_0 _S22;

#line 102
    (&_S22)->position_3 = output_2.position_0;

#line 102
    return _S22;
}

