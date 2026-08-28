#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 62 "shaders/exposure.slang"
struct ExposureParams_0
{
    uint viewport_x_0;
    uint viewport_y_0;
    float brighten_blend_0;
    float darken_blend_0;
};


#line 290
struct KernelContext_0
{
    atomic<uint> device* histogram_0;
    float device* previous_0;
    ExposureParams_0 constant* params_0;
    float device* measured_0;
    texture2d<float, access::sample> scene_0;
};


#line 197
[[kernel]] void clearMain(uint3 thread_0 [[thread_position_in_grid]], atomic<uint> device* histogram_1 [[buffer(1)]], float device* previous_1 [[buffer(3)]], ExposureParams_0 constant* params_1 [[buffer(0)]], float device* measured_1 [[buffer(2)]], texture2d<float, access::sample> scene_1 [[texture(0)]])
{

#line 197
    thread KernelContext_0 kernelContext_0;

#line 197
    (&kernelContext_0)->histogram_0 = histogram_1;

#line 197
    (&kernelContext_0)->previous_0 = previous_1;

#line 197
    (&kernelContext_0)->params_0 = params_1;

#line 197
    (&kernelContext_0)->measured_0 = measured_1;

#line 197
    (&kernelContext_0)->scene_0 = scene_1;

    uint _S1 = thread_0.x;

#line 199
    if(_S1 >= 96U)
    {
        return;
    }
    atomic_store_explicit((&kernelContext_0)->histogram_0+_S1, 0U, memory_order_relaxed);
    return;
}


#line 213
float bin_luminance_0(uint bin_0)
{


    return (as_type<float>(((uint(int(-12) + int(bin_0 / 4U) + int(127)) << 23U) | ((bin_0 % 4U) << 21U))));
}


#line 251
[[kernel]] void reduceMain(atomic<uint> device* histogram_2 [[buffer(1)]], float device* previous_2 [[buffer(3)]], ExposureParams_0 constant* params_2 [[buffer(0)]], float device* measured_2 [[buffer(2)]], texture2d<float, access::sample> scene_2 [[texture(0)]])
{

#line 251
    thread KernelContext_0 kernelContext_1;

#line 251
    (&kernelContext_1)->histogram_0 = histogram_2;

#line 251
    (&kernelContext_1)->previous_0 = previous_2;

#line 251
    (&kernelContext_1)->params_0 = params_2;

#line 251
    (&kernelContext_1)->measured_0 = measured_2;

#line 251
    (&kernelContext_1)->scene_0 = scene_2;

#line 251
    uint bin_1 = 1U;

#line 251
    uint total_0 = 0U;



    for(;;)
    {

#line 255
        if(bin_1 < 96U)
        {
        }
        else
        {

#line 255
            break;
        }
        uint _S2 = atomic_load_explicit((&kernelContext_1)->histogram_0+bin_1, memory_order_relaxed);

#line 257
        uint total_1 = total_0 + _S2;

#line 255
        bin_1 = bin_1 + 1U;

#line 255
        total_0 = total_1;

#line 255
    }

#line 255
    float rate_0;

#line 255
    float target_0;



    if(total_0 > 0U)
    {
        float _S3 = float(total_0);

#line 261
        uint _S4 = uint(_S3 * 0.5f);
        uint _S5 = uint(_S3 * 0.94999998807907104f);

#line 262
        bin_1 = 1U;

#line 262
        uint seen_0 = 0U;

#line 262
        rate_0 = 0.0f;

#line 262
        float population_0 = 0.0f;



        for(;;)
        {

#line 266
            if(bin_1 < 96U)
            {
            }
            else
            {

#line 266
                break;
            }

            uint _S6 = atomic_load_explicit((&kernelContext_1)->histogram_0+bin_1, memory_order_relaxed);

#line 269
            uint seen_1 = seen_0 + _S6;
            uint _S7 = max(seen_0, _S4);
            uint _S8 = min(seen_1, _S5);
            if(_S8 > _S7)
            {
                float part_0 = float(_S8 - _S7);

                float population_1 = population_0 + part_0;

#line 276
                rate_0 = rate_0 + part_0 * bin_luminance_0(bin_1) * 1.09050774574279785f;

#line 276
                population_0 = population_1;

#line 272
            }

#line 266
            bin_1 = bin_1 + 1U;

#line 266
            seen_0 = seen_1;

#line 266
        }

#line 279
        if(population_0 > 0.0f)
        {

#line 279
            target_0 = clamp(0.18000000715255737f / (rate_0 / population_0), 0.03125f, 32.0f);

#line 279
        }
        else
        {

#line 279
            target_0 = 1.0f;

#line 279
        }

#line 259
    }
    else
    {

#line 259
        target_0 = 1.0f;

#line 259
    }

#line 285
    float prior_0 = (&kernelContext_1)->previous_0[int(0)];
    if(target_0 > prior_0)
    {

#line 286
        rate_0 = (&kernelContext_1)->params_0->brighten_blend_0;

#line 286
    }
    else
    {

#line 286
        rate_0 = (&kernelContext_1)->params_0->darken_blend_0;

#line 286
    }
    float blend_0 = clamp(rate_0, 0.0f, 1.0f);
    if(blend_0 >= 1.0f)
    {
        *((&kernelContext_1)->measured_0+int(0)) = target_0;

#line 288
    }
    else
    {

        if(blend_0 <= 0.0f)
        {
            *((&kernelContext_1)->measured_0+int(0)) = prior_0;

#line 292
        }
        else
        {



            *((&kernelContext_1)->measured_0+int(0)) = clamp(prior_0 + (target_0 - prior_0) * blend_0, 0.03125f, 32.0f);

#line 292
        }

#line 288
    }

#line 300
    return;
}


#line 162
float luma_0(float3 color_0)
{
    return dot(color_0, float3(0.2125999927520752f, 0.71520000696182251f, 0.07220000028610229f));
}


#line 177
uint bin_of_0(float luminance_0)
{
    uint bits_0 = (as_type<uint>((luminance_0)));

#line 187
    return uint(clamp((int(bits_0 >> 23U) - int(127) - int(-12)) * int(4) + int((bits_0 >> 21U) & 3U), int(0), int(95)));
}


#line 320
[[kernel]] void histogramMain(uint3 thread_1 [[thread_position_in_grid]], atomic<uint> device* histogram_3 [[buffer(1)]], float device* previous_3 [[buffer(3)]], ExposureParams_0 constant* params_3 [[buffer(0)]], float device* measured_3 [[buffer(2)]], texture2d<float, access::sample> scene_3 [[texture(0)]])
{

#line 320
    thread KernelContext_0 kernelContext_2;

#line 320
    (&kernelContext_2)->histogram_0 = histogram_3;

#line 320
    (&kernelContext_2)->previous_0 = previous_3;

#line 320
    (&kernelContext_2)->params_0 = params_3;

#line 320
    (&kernelContext_2)->measured_0 = measured_3;

#line 320
    (&kernelContext_2)->scene_0 = scene_3;

    uint index_0 = thread_1.x;
    if(index_0 >= (params_3->viewport_x_0 * params_3->viewport_y_0))
    {
        return;
    }
    uint _S9 = index_0 % params_3->viewport_x_0;

#line 327
    uint _S10 = index_0 / params_3->viewport_x_0;
    int3 _S11 = int3(int2(uint2(_S9, _S10)), int(0));
    uint _S12 = atomic_fetch_add_explicit((&kernelContext_2)->histogram_0+bin_of_0(luma_0((((&kernelContext_2)->scene_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z))).xyz)), 1U, memory_order_relaxed);
    return;
}

