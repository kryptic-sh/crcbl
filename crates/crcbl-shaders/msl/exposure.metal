#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 56 "shaders/exposure.slang"
struct ExposureParams_0
{
    uint viewport_x_0;
    uint viewport_y_0;
    uint pad_0_0;
    uint pad_1_0;
};


#line 56
struct KernelContext_0
{
    atomic<uint> device* histogram_0;
    float device* measured_0;
    ExposureParams_0 constant* params_0;
    texture2d<float, access::sample> scene_0;
};


#line 179
[[kernel]] void clearMain(uint3 thread_0 [[thread_position_in_grid]], atomic<uint> device* histogram_1 [[buffer(1)]], float device* measured_1 [[buffer(2)]], ExposureParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> scene_1 [[texture(0)]])
{

#line 179
    thread KernelContext_0 kernelContext_0;

#line 179
    (&kernelContext_0)->histogram_0 = histogram_1;

#line 179
    (&kernelContext_0)->measured_0 = measured_1;

#line 179
    (&kernelContext_0)->params_0 = params_1;

#line 179
    (&kernelContext_0)->scene_0 = scene_1;

    uint _S1 = thread_0.x;

#line 181
    if(_S1 >= 96U)
    {
        return;
    }
    atomic_store_explicit((&kernelContext_0)->histogram_0+_S1, 0U, memory_order_relaxed);
    return;
}


#line 195
float bin_luminance_0(uint bin_0)
{


    return (as_type<float>(((uint(int(-12) + int(bin_0 / 4U) + int(127)) << 23U) | ((bin_0 % 4U) << 21U))));
}


#line 220
[[kernel]] void reduceMain(atomic<uint> device* histogram_2 [[buffer(1)]], float device* measured_2 [[buffer(2)]], ExposureParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> scene_2 [[texture(0)]])
{

#line 220
    thread KernelContext_0 kernelContext_1;

#line 220
    (&kernelContext_1)->histogram_0 = histogram_2;

#line 220
    (&kernelContext_1)->measured_0 = measured_2;

#line 220
    (&kernelContext_1)->params_0 = params_2;

#line 220
    (&kernelContext_1)->scene_0 = scene_2;

#line 220
    uint bin_1 = 1U;

#line 220
    uint total_0 = 0U;


    for(;;)
    {

#line 223
        if(bin_1 < 96U)
        {
        }
        else
        {

#line 223
            break;
        }
        uint _S2 = atomic_load_explicit((&kernelContext_1)->histogram_0+bin_1, memory_order_relaxed);

#line 225
        uint total_1 = total_0 + _S2;

#line 223
        bin_1 = bin_1 + 1U;

#line 223
        total_0 = total_1;

#line 223
    }



    if(total_0 == 0U)
    {
        *((&kernelContext_1)->measured_0+int(0)) = 1.0f;
        return;
    }

    float _S3 = float(total_0);

#line 233
    uint _S4 = uint(_S3 * 0.5f);
    uint _S5 = uint(_S3 * 0.94999998807907104f);

#line 234
    bin_1 = 1U;

#line 234
    uint seen_0 = 0U;

#line 234
    float weighted_0 = 0.0f;

#line 234
    float population_0 = 0.0f;



    for(;;)
    {

#line 238
        if(bin_1 < 96U)
        {
        }
        else
        {

#line 238
            break;
        }

        uint _S6 = atomic_load_explicit((&kernelContext_1)->histogram_0+bin_1, memory_order_relaxed);

#line 241
        uint seen_1 = seen_0 + _S6;
        uint _S7 = max(seen_0, _S4);
        uint _S8 = min(seen_1, _S5);
        if(_S8 > _S7)
        {
            float part_0 = float(_S8 - _S7);

            float population_1 = population_0 + part_0;

#line 248
            weighted_0 = weighted_0 + part_0 * bin_luminance_0(bin_1) * 1.09050774574279785f;

#line 248
            population_0 = population_1;

#line 244
        }

#line 238
        bin_1 = bin_1 + 1U;

#line 238
        seen_0 = seen_1;

#line 238
    }

#line 251
    if(population_0 == 0.0f)
    {
        *((&kernelContext_1)->measured_0+int(0)) = 1.0f;
        return;
    }
    *((&kernelContext_1)->measured_0+int(0)) = clamp(0.18000000715255737f / (weighted_0 / population_0), 0.03125f, 32.0f);
    return;
}


#line 144
float luma_0(float3 color_0)
{
    return dot(color_0, float3(0.2125999927520752f, 0.71520000696182251f, 0.07220000028610229f));
}


#line 159
uint bin_of_0(float luminance_0)
{
    uint bits_0 = (as_type<uint>((luminance_0)));

#line 169
    return uint(clamp((int(bits_0 >> 23U) - int(127) - int(-12)) * int(4) + int((bits_0 >> 21U) & 3U), int(0), int(95)));
}


#line 277
[[kernel]] void histogramMain(uint3 thread_1 [[thread_position_in_grid]], atomic<uint> device* histogram_3 [[buffer(1)]], float device* measured_3 [[buffer(2)]], ExposureParams_0 constant* params_3 [[buffer(0)]], texture2d<float, access::sample> scene_3 [[texture(0)]])
{

#line 277
    thread KernelContext_0 kernelContext_2;

#line 277
    (&kernelContext_2)->histogram_0 = histogram_3;

#line 277
    (&kernelContext_2)->measured_0 = measured_3;

#line 277
    (&kernelContext_2)->params_0 = params_3;

#line 277
    (&kernelContext_2)->scene_0 = scene_3;

    uint index_0 = thread_1.x;
    if(index_0 >= (params_3->viewport_x_0 * params_3->viewport_y_0))
    {
        return;
    }
    uint _S9 = index_0 % params_3->viewport_x_0;

#line 284
    uint _S10 = index_0 / params_3->viewport_x_0;
    int3 _S11 = int3(int2(uint2(_S9, _S10)), int(0));
    uint _S12 = atomic_fetch_add_explicit((&kernelContext_2)->histogram_0+bin_of_0(luma_0((((&kernelContext_2)->scene_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z))).xyz)), 1U, memory_order_relaxed);
    return;
}

