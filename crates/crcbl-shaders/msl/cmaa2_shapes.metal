#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 83 "shaders/cmaa2_shapes.slang"
struct Cmaa2Params_0
{
    uint viewport_x_0;
    uint viewport_y_0;
};


#line 281
struct KernelContext_0
{
    Cmaa2Params_0 constant* params_0;
    uint device* edges_0;
    texture2d<float, access::sample> source_0;
    atomic<uint> device* accum_0;
};


#line 171
uint edge_at_0(uint x_0, uint y_0, KernelContext_0 thread* kernelContext_0)
{

#line 171
    bool _S1;

    if(x_0 >= (kernelContext_0->params_0->viewport_x_0))
    {

#line 173
        _S1 = true;

#line 173
    }
    else
    {

#line 173
        _S1 = y_0 >= (kernelContext_0->params_0->viewport_y_0);

#line 173
    }

#line 173
    if(_S1)
    {
        return 0U;
    }
    return *(kernelContext_0->edges_0+(y_0 * kernelContext_0->params_0->viewport_x_0 + x_0));
}


#line 171
uint edge_at_1(uint x_1, uint y_1, KernelContext_0 thread* kernelContext_1)
{

#line 171
    bool _S2;

    if(x_1 >= (kernelContext_1->params_0->viewport_x_0))
    {

#line 173
        _S2 = true;

#line 173
    }
    else
    {

#line 173
        _S2 = y_1 >= (kernelContext_1->params_0->viewport_y_0);

#line 173
    }

#line 173
    if(_S2)
    {
        return 0U;
    }
    return *(kernelContext_1->edges_0+(y_1 * kernelContext_1->params_0->viewport_x_0 + x_1));
}


#line 188
int horizontal_turn_0(uint column_0, uint row_0, KernelContext_0 thread* kernelContext_2)
{

#line 188
    uint _S3 = edge_at_1(column_0, row_0, kernelContext_2);

    bool below_0 = (_S3 & 1U) != 0U;

#line 190
    bool above_0;
    if(row_0 > 0U)
    {

#line 191
        uint _S4 = edge_at_1(column_0, row_0 - 1U, kernelContext_2);

#line 191
        above_0 = (_S4 & 1U) != 0U;

#line 191
    }
    else
    {

#line 191
        above_0 = false;

#line 191
    }

#line 191
    bool _S5;
    if(above_0)
    {

#line 192
        _S5 = !below_0;

#line 192
    }
    else
    {

#line 192
        _S5 = false;

#line 192
    }

#line 192
    if(_S5)
    {
        return int(1);
    }
    if(below_0)
    {

#line 196
        above_0 = !above_0;

#line 196
    }
    else
    {

#line 196
        above_0 = false;

#line 196
    }

#line 196
    if(above_0)
    {
        return int(-1);
    }
    return int(0);
}


#line 232
float boundary_height_0(float t_0, int start_0, int end_0, bool u_shape_0)
{
    if(u_shape_0)
    {
        return -0.5f * float(start_0) * (2.0f * abs(t_0 - 0.5f)) * 0.5f;
    }
    return -0.5f * float(start_0) * (1.0f - t_0) - 0.5f * float(end_0) * t_0;
}


#line 262
void accumulate_0(uint target_0, uint from_0, int2 from_texel_0, float share_0, KernelContext_0 thread* kernelContext_3)
{
    if(share_0 < 0.001953125f)
    {
        return;
    }
    uint pixels_0 = kernelContext_3->params_0->viewport_x_0 * kernelContext_3->params_0->viewport_y_0;

#line 268
    bool _S6;
    if(target_0 >= pixels_0)
    {

#line 269
        _S6 = true;

#line 269
    }
    else
    {

#line 269
        _S6 = from_0 >= pixels_0;

#line 269
    }

#line 269
    if(_S6)
    {
        return;
    }

#line 277
    int3 _S7 = int3(from_texel_0, int(0));

#line 277
    float3 color_0 = saturate(((kernelContext_3->source_0).read(vec<uint,2>(((_S7)).xy), uint(((_S7)).z))).xyz);
    uint weight_0 = uint(min(share_0, 0.5f) * 1.048576e+06f);
    float scaled_0 = float(weight_0);

    uint _S8 = target_0 * 4U;

#line 281
    uint _S9 = atomic_fetch_add_explicit(kernelContext_3->accum_0+_S8, uint(color_0.x * scaled_0), memory_order_relaxed);
    uint _S10 = atomic_fetch_add_explicit(kernelContext_3->accum_0+(_S8 + 1U), uint(color_0.y * scaled_0), memory_order_relaxed);
    uint _S11 = atomic_fetch_add_explicit(kernelContext_3->accum_0+(_S8 + 2U), uint(color_0.z * scaled_0), memory_order_relaxed);
    uint _S12 = atomic_fetch_add_explicit(kernelContext_3->accum_0+(_S8 + 3U), weight_0, memory_order_relaxed);
    return;
}


#line 309
void blend_line_0(uint first_0, uint stride_0, uint len_0, uint offset_0, int start_1, int end_1, int2 first_texel_0, int2 texel_step_0, int2 across_0, KernelContext_0 thread* kernelContext_4)
{

#line 318
    bool _S13;

    if(start_1 == int(0))
    {

#line 320
        _S13 = true;

#line 320
    }
    else
    {

#line 320
        _S13 = end_1 == int(0);

#line 320
    }

#line 320
    if(_S13)
    {
        return;
    }
    float _S14 = 1.0f / float(len_0);
    bool _S15 = start_1 == end_1;

#line 325
    uint i_0 = 0U;
    for(;;)
    {

#line 326
        if(i_0 < len_0)
        {
        }
        else
        {

#line 326
            break;
        }

#line 333
        float near_0 = boundary_height_0(float(i_0) * _S14, start_1, end_1, _S15);
        uint _S16 = i_0 + 1U;

#line 334
        float far_0 = boundary_height_0(float(_S16) * _S14, start_1, end_1, _S15);

#line 341
        if(near_0 <= 0.0f)
        {

#line 341
            _S13 = far_0 <= 0.0f;

#line 341
        }
        else
        {

#line 341
            _S13 = false;

#line 341
        }

#line 341
        float below_1;

#line 341
        float above_1;

#line 341
        if(_S13)
        {

#line 341
            below_1 = -0.5f * (near_0 + far_0);

#line 341
            above_1 = 0.0f;

#line 341
        }
        else
        {

#line 341
            bool _S17;

#line 346
            if(near_0 >= 0.0f)
            {

#line 346
                _S17 = far_0 >= 0.0f;

#line 346
            }
            else
            {

#line 346
                _S17 = false;

#line 346
            }

#line 346
            if(_S17)
            {

                float _S18 = 0.5f * (near_0 + far_0);

#line 349
                below_1 = 0.0f;

#line 349
                above_1 = _S18;

#line 346
            }
            else
            {

#line 353
                float crossing_0 = near_0 / (near_0 - far_0);
                float _S19 = 0.5f * abs(min(near_0, far_0));

#line 354
                if(near_0 < 0.0f)
                {

#line 354
                    below_1 = crossing_0;

#line 354
                }
                else
                {

#line 354
                    below_1 = 1.0f - crossing_0;

#line 354
                }

#line 354
                float _S20 = _S19 * below_1;
                float _S21 = 0.5f * max(near_0, far_0);

#line 355
                if(near_0 > 0.0f)
                {

#line 355
                    above_1 = crossing_0;

#line 355
                }
                else
                {

#line 355
                    above_1 = 1.0f - crossing_0;

#line 355
                }

#line 355
                float _S22 = _S21 * above_1;

#line 355
                below_1 = _S20;

#line 355
                above_1 = _S22;

#line 346
            }

#line 341
        }

#line 358
        uint pixel_0 = first_0 + i_0 * stride_0;

#line 367
        int2 texel_0 = first_texel_0 + int2(int(i_0))  * texel_step_0;
        uint _S23 = pixel_0 - offset_0;

#line 368
        accumulate_0(_S23, pixel_0, texel_0, below_1, kernelContext_4);

#line 368
        accumulate_0(pixel_0, _S23, texel_0 - across_0, above_1, kernelContext_4);

#line 326
        i_0 = _S16;

#line 326
    }

#line 371
    return;
}


#line 206
int vertical_turn_0(uint column_1, uint row_1, KernelContext_0 thread* kernelContext_5)
{

#line 206
    uint _S24 = edge_at_1(column_1, row_1, kernelContext_5);

    bool right_0 = (_S24 & 2U) != 0U;

#line 208
    bool left_0;
    if(column_1 > 0U)
    {

#line 209
        uint _S25 = edge_at_1(column_1 - 1U, row_1, kernelContext_5);

#line 209
        left_0 = (_S25 & 2U) != 0U;

#line 209
    }
    else
    {

#line 209
        left_0 = false;

#line 209
    }

#line 209
    bool _S26;
    if(left_0)
    {

#line 210
        _S26 = !right_0;

#line 210
    }
    else
    {

#line 210
        _S26 = false;

#line 210
    }

#line 210
    if(_S26)
    {
        return int(1);
    }
    if(right_0)
    {

#line 214
        left_0 = !left_0;

#line 214
    }
    else
    {

#line 214
        left_0 = false;

#line 214
    }

#line 214
    if(left_0)
    {
        return int(-1);
    }
    return int(0);
}


#line 383
[[kernel]] void shapesMain(uint3 thread_0 [[thread_position_in_grid]], Cmaa2Params_0 constant* params_1 [[buffer(0)]], uint device* edges_1 [[buffer(1)]], texture2d<float, access::sample> source_1 [[texture(0)]], atomic<uint> device* accum_1 [[buffer(2)]])
{

#line 383
    bool _S27;

#line 383
    bool _S28;

#line 383
    thread KernelContext_0 kernelContext_6;

#line 383
    (&kernelContext_6)->params_0 = params_1;

#line 383
    (&kernelContext_6)->edges_0 = edges_1;

#line 383
    (&kernelContext_6)->source_0 = source_1;

#line 383
    (&kernelContext_6)->accum_0 = accum_1;

    uint index_0 = thread_0.x;
    uint width_0 = params_1->viewport_x_0;

    if(index_0 >= (params_1->viewport_x_0 * params_1->viewport_y_0))
    {
        return;
    }

    uint device* _S29 = (&kernelContext_6)->edges_0+index_0;

#line 393
    uint own_0 = *_S29;
    if((*_S29) == 0U)
    {
        return;
    }

    uint x_2 = index_0 % width_0;
    uint y_2 = index_0 / width_0;

#line 400
    bool _S30;



    if((own_0 & 2U) != 0U)
    {

#line 404
        uint _S31 = edge_at_0(x_2 - 1U, y_2, &kernelContext_6);

#line 404
        _S30 = (_S31 & 2U) == 0U;

#line 404
    }
    else
    {

#line 404
        _S30 = false;

#line 404
    }

#line 404
    uint len_1;

#line 404
    if(_S30)
    {

#line 404
        len_1 = 1U;


        for(;;)
        {

#line 407
            bool _S32 = len_1 <= 64U;

#line 407
            _S27 = _S32;

#line 407
            if(_S32)
            {

#line 407
                uint _S33 = edge_at_0(x_2 + len_1, y_2, &kernelContext_6);

#line 407
                _S30 = (_S33 & 2U) != 0U;

#line 407
            }
            else
            {

#line 407
                _S30 = false;

#line 407
            }

#line 407
            if(_S30)
            {
            }
            else
            {

#line 407
                break;
            }

#line 407
            len_1 = len_1 + 1U;

#line 407
        }



        if(_S27)
        {

#line 411
            int _S34 = horizontal_turn_0(x_2, y_2, &kernelContext_6);

#line 411
            int _S35 = horizontal_turn_0(x_2 + len_1, y_2, &kernelContext_6);

#line 411
            blend_line_0(index_0, 1U, len_1, width_0, _S34, _S35, int2(int(x_2), int(y_2)), int2(int(1), int(0)), int2(int(0), int(1)), &kernelContext_6);

#line 411
        }

#line 404
    }

#line 428
    if((own_0 & 1U) != 0U)
    {

#line 428
        uint _S36 = edge_at_0(x_2, y_2 - 1U, &kernelContext_6);

#line 428
        _S30 = (_S36 & 1U) == 0U;

#line 428
    }
    else
    {

#line 428
        _S30 = false;

#line 428
    }

#line 428
    if(_S30)
    {

#line 428
        len_1 = 1U;


        for(;;)
        {

#line 431
            bool _S37 = len_1 <= 64U;

#line 431
            _S28 = _S37;

#line 431
            if(_S37)
            {

#line 431
                uint _S38 = edge_at_0(x_2, y_2 + len_1, &kernelContext_6);

#line 431
                _S30 = (_S38 & 1U) != 0U;

#line 431
            }
            else
            {

#line 431
                _S30 = false;

#line 431
            }

#line 431
            if(_S30)
            {
            }
            else
            {

#line 431
                break;
            }

#line 431
            len_1 = len_1 + 1U;

#line 431
        }



        if(_S28)
        {

#line 435
            int _S39 = vertical_turn_0(x_2, y_2, &kernelContext_6);

#line 435
            int _S40 = vertical_turn_0(x_2, y_2 + len_1, &kernelContext_6);

#line 435
            blend_line_0(index_0, width_0, len_1, 1U, _S39, _S40, int2(int(x_2), int(y_2)), int2(int(0), int(1)), int2(int(1), int(0)), &kernelContext_6);

#line 435
        }

#line 428
    }

#line 449
    return;
}

