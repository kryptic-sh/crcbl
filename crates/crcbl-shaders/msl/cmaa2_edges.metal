#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 180 "shaders/cmaa2_edges.slang"
float luma_of_0(float3 color_0)
{
    return sqrt(dot(color_0, float3(0.2125999927520752f, 0.71520000696182251f, 0.07220000028610229f)));
}


#line 99
struct Cmaa2Params_0
{
    uint viewport_x_0;
    uint viewport_y_0;
};


#line 248
struct KernelContext_0
{
    Cmaa2Params_0 constant* params_0;
    atomic<uint> device* accum_0;
    texture2d<float, access::sample> source_0;
    uint device* edges_0;
};


#line 190
float luma_at_0(int2 texel_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(texel_0, int2(int(0), int(0)), int2(int(kernelContext_0->params_0->viewport_x_0) - int(1), int(kernelContext_0->params_0->viewport_y_0) - int(1))), int(0));

#line 193
    return luma_of_0(((kernelContext_0->source_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z))).xyz);
}


#line 204
[[kernel]] void edgesMain(uint3 thread_0 [[thread_position_in_grid]], Cmaa2Params_0 constant* params_1 [[buffer(0)]], atomic<uint> device* accum_1 [[buffer(2)]], texture2d<float, access::sample> source_1 [[texture(0)]], uint device* edges_1 [[buffer(1)]])
{

#line 204
    thread KernelContext_0 kernelContext_1;

#line 204
    (&kernelContext_1)->params_0 = params_1;

#line 204
    (&kernelContext_1)->accum_0 = accum_1;

#line 204
    (&kernelContext_1)->source_0 = source_1;

#line 204
    (&kernelContext_1)->edges_0 = edges_1;

    uint index_0 = thread_0.x;

    if(index_0 >= (params_1->viewport_x_0 * params_1->viewport_y_0))
    {
        return;
    }
    uint x_0 = index_0 % params_1->viewport_x_0;
    uint y_0 = index_0 / params_1->viewport_x_0;
    int2 texel_1 = int2(int(x_0), int(y_0));

#line 214
    uint word_0 = 0U;



    for(;;)
    {

#line 218
        if(word_0 < 4U)
        {
        }
        else
        {

#line 218
            break;
        }
        atomic_store_explicit((&kernelContext_1)->accum_0+(index_0 * 4U + word_0), 0U, memory_order_relaxed);

#line 218
        word_0 = word_0 + 1U;

#line 218
    }

#line 218
    float _S2 = luma_at_0(texel_1, &kernelContext_1);

#line 218
    float _S3 = luma_at_0(texel_1 + int2(int(-1), int(0)), &kernelContext_1);

#line 218
    float _S4 = luma_at_0(texel_1 + int2(int(0), int(-1)), &kernelContext_1);

#line 218
    float _S5 = luma_at_0(texel_1 + int2(int(1), int(0)), &kernelContext_1);

#line 218
    float _S6 = luma_at_0(texel_1 + int2(int(0), int(1)), &kernelContext_1);

#line 218
    float2 _S7 = float2(_S2) ;

#line 230
    float2 delta_0 = abs(_S7 - float2(_S3, _S4));

#line 235
    float2 other_0 = abs(_S7 - float2(_S5, _S6));
    float _S8 = max(max(delta_0.x, delta_0.y), max(other_0.x, other_0.y));
    float2 marked_0 = step(float2(0.10000000149011612f, 0.10000000149011612f), delta_0) * step(float2(_S8, _S8), float2(2.0f)  * delta_0);

#line 237
    bool _S9;


    if(x_0 > 0U)
    {

#line 240
        _S9 = (marked_0.x) > 0.0f;

#line 240
    }
    else
    {

#line 240
        _S9 = false;

#line 240
    }

#line 240
    uint bits_0;

#line 240
    if(_S9)
    {

#line 240
        bits_0 = 1U;

#line 240
    }
    else
    {

#line 240
        bits_0 = 0U;

#line 240
    }



    if(y_0 > 0U)
    {

#line 244
        _S9 = (marked_0.y) > 0.0f;

#line 244
    }
    else
    {

#line 244
        _S9 = false;

#line 244
    }

#line 244
    if(_S9)
    {

#line 244
        bits_0 = bits_0 | 2U;

#line 244
    }



    *((&kernelContext_1)->edges_0+index_0) = bits_0;
    return;
}

