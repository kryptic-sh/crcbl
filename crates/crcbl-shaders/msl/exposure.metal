#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 74 "shaders/exposure.slang"
struct ExposureParams_0
{
    uint viewport_x_0;
    uint viewport_y_0;
    float brighten_blend_0;
    float darken_blend_0;
};


#line 302
struct KernelContext_0
{
    atomic<uint> device* histogram_0;
    float device* previous_0;
    ExposureParams_0 constant* params_0;
    float device* measured_0;
    texture2d<float, access::sample> scene_0;
};


#line 209
[[kernel]] void clearMain(uint3 thread_0 [[thread_position_in_grid]], atomic<uint> device* histogram_1 [[buffer(1)]], float device* previous_1 [[buffer(3)]], ExposureParams_0 constant* params_1 [[buffer(0)]], float device* measured_1 [[buffer(2)]], texture2d<float, access::sample> scene_1 [[texture(0)]])
{

#line 209
    thread KernelContext_0 kernelContext_0;

#line 209
    (&kernelContext_0)->histogram_0 = histogram_1;

#line 209
    (&kernelContext_0)->previous_0 = previous_1;

#line 209
    (&kernelContext_0)->params_0 = params_1;

#line 209
    (&kernelContext_0)->measured_0 = measured_1;

#line 209
    (&kernelContext_0)->scene_0 = scene_1;

    uint _S1 = thread_0.x;

#line 211
    if(_S1 >= 96U)
    {
        return;
    }
    atomic_store_explicit((&kernelContext_0)->histogram_0+_S1, 0U, memory_order_relaxed);
    return;
}


#line 225
float bin_luminance_0(uint bin_0)
{


    return (as_type<float>(((uint(int(-12) + int(bin_0 / 4U) + int(127)) << 23U) | ((bin_0 % 4U) << 21U))));
}


#line 263
[[kernel]] void reduceMain(atomic<uint> device* histogram_2 [[buffer(1)]], float device* previous_2 [[buffer(3)]], ExposureParams_0 constant* params_2 [[buffer(0)]], float device* measured_2 [[buffer(2)]], texture2d<float, access::sample> scene_2 [[texture(0)]])
{

#line 263
    thread KernelContext_0 kernelContext_1;

#line 263
    (&kernelContext_1)->histogram_0 = histogram_2;

#line 263
    (&kernelContext_1)->previous_0 = previous_2;

#line 263
    (&kernelContext_1)->params_0 = params_2;

#line 263
    (&kernelContext_1)->measured_0 = measured_2;

#line 263
    (&kernelContext_1)->scene_0 = scene_2;

#line 263
    uint bin_1 = 1U;

#line 263
    uint total_0 = 0U;



    for(;;)
    {

#line 267
        if(bin_1 < 96U)
        {
        }
        else
        {

#line 267
            break;
        }
        uint _S2 = atomic_load_explicit((&kernelContext_1)->histogram_0+bin_1, memory_order_relaxed);

#line 269
        uint total_1 = total_0 + _S2;

#line 267
        bin_1 = bin_1 + 1U;

#line 267
        total_0 = total_1;

#line 267
    }

#line 267
    float rate_0;

#line 267
    float target_0;



    if(total_0 > 0U)
    {
        float _S3 = float(total_0);

#line 273
        uint _S4 = uint(_S3 * 0.5f);
        uint _S5 = uint(_S3 * 0.94999998807907104f);

#line 274
        bin_1 = 1U;

#line 274
        uint seen_0 = 0U;

#line 274
        rate_0 = 0.0f;

#line 274
        float population_0 = 0.0f;



        for(;;)
        {

#line 278
            if(bin_1 < 96U)
            {
            }
            else
            {

#line 278
                break;
            }

            uint _S6 = atomic_load_explicit((&kernelContext_1)->histogram_0+bin_1, memory_order_relaxed);

#line 281
            uint seen_1 = seen_0 + _S6;
            uint _S7 = max(seen_0, _S4);
            uint _S8 = min(seen_1, _S5);
            if(_S8 > _S7)
            {
                float part_0 = float(_S8 - _S7);

                float population_1 = population_0 + part_0;

#line 288
                rate_0 = rate_0 + part_0 * bin_luminance_0(bin_1) * 1.09050774574279785f;

#line 288
                population_0 = population_1;

#line 284
            }

#line 278
            bin_1 = bin_1 + 1U;

#line 278
            seen_0 = seen_1;

#line 278
        }

#line 291
        if(population_0 > 0.0f)
        {

#line 291
            target_0 = clamp(0.18000000715255737f / (rate_0 / population_0), 0.03125f, 32.0f);

#line 291
        }
        else
        {

#line 291
            target_0 = 1.0f;

#line 291
        }

#line 271
    }
    else
    {

#line 271
        target_0 = 1.0f;

#line 271
    }

#line 297
    float prior_0 = (&kernelContext_1)->previous_0[int(0)];
    if(target_0 > prior_0)
    {

#line 298
        rate_0 = (&kernelContext_1)->params_0->brighten_blend_0;

#line 298
    }
    else
    {

#line 298
        rate_0 = (&kernelContext_1)->params_0->darken_blend_0;

#line 298
    }
    float blend_0 = clamp(rate_0, 0.0f, 1.0f);
    if(blend_0 >= 1.0f)
    {
        *((&kernelContext_1)->measured_0+int(0)) = target_0;

#line 300
    }
    else
    {

        if(blend_0 <= 0.0f)
        {
            *((&kernelContext_1)->measured_0+int(0)) = prior_0;

#line 304
        }
        else
        {



            *((&kernelContext_1)->measured_0+int(0)) = clamp(prior_0 + (target_0 - prior_0) * blend_0, 0.03125f, 32.0f);

#line 304
        }

#line 300
    }

#line 312
    return;
}


#line 174
float luma_0(float3 color_0)
{
    return dot(color_0, float3(0.2125999927520752f, 0.71520000696182251f, 0.07220000028610229f));
}


#line 189
uint bin_of_0(float luminance_0)
{
    uint bits_0 = (as_type<uint>((luminance_0)));

#line 199
    return uint(clamp((int(bits_0 >> 23U) - int(127) - int(-12)) * int(4) + int((bits_0 >> 21U) & 3U), int(0), int(95)));
}


#line 330
[[kernel]] void histogramMain(uint3 thread_1 [[thread_position_in_grid]], atomic<uint> device* histogram_3 [[buffer(1)]], float device* previous_3 [[buffer(3)]], ExposureParams_0 constant* params_3 [[buffer(0)]], float device* measured_3 [[buffer(2)]], texture2d<float, access::sample> scene_3 [[texture(0)]])
{

#line 330
    thread KernelContext_0 kernelContext_2;

#line 330
    (&kernelContext_2)->histogram_0 = histogram_3;

#line 330
    (&kernelContext_2)->previous_0 = previous_3;

#line 330
    (&kernelContext_2)->params_0 = params_3;

#line 330
    (&kernelContext_2)->measured_0 = measured_3;

#line 330
    (&kernelContext_2)->scene_0 = scene_3;

    uint2 texel_0 = thread_1.xy;

#line 332
    bool _S9;
    if((texel_0.x) >= (params_3->viewport_x_0))
    {

#line 333
        _S9 = true;

#line 333
    }
    else
    {

#line 333
        _S9 = (texel_0.y) >= ((&kernelContext_2)->params_0->viewport_y_0);

#line 333
    }

#line 333
    if(_S9)
    {
        return;
    }
    int3 _S10 = int3(int2(texel_0), int(0));
    uint _S11 = atomic_fetch_add_explicit((&kernelContext_2)->histogram_0+bin_of_0(luma_0((((&kernelContext_2)->scene_0).read(vec<uint,2>(((_S10)).xy), uint(((_S10)).z))).xyz)), 1U, memory_order_relaxed);
    return;
}

