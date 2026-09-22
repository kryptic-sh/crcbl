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


#line 283
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
void accumulate_0(uint target_0, uint from_0, float share_0, KernelContext_0 thread* kernelContext_3)
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

    uint width_0 = kernelContext_3->params_0->viewport_x_0;
    uint _S7 = from_0 % kernelContext_3->params_0->viewport_x_0;

#line 275
    int _S8 = int(_S7);

#line 275
    uint _S9 = from_0 / width_0;



    int3 _S10 = int3(int2(_S8, int(_S9)), int(0));

#line 279
    float3 color_0 = saturate(((kernelContext_3->source_0).read(vec<uint,2>(((_S10)).xy), uint(((_S10)).z))).xyz);
    uint weight_0 = uint(min(share_0, 0.5f) * 1.048576e+06f);
    float scaled_0 = float(weight_0);

    uint _S11 = target_0 * 4U;

#line 283
    uint _S12 = atomic_fetch_add_explicit(kernelContext_3->accum_0+_S11, uint(color_0.x * scaled_0), memory_order_relaxed);
    uint _S13 = atomic_fetch_add_explicit(kernelContext_3->accum_0+(_S11 + 1U), uint(color_0.y * scaled_0), memory_order_relaxed);
    uint _S14 = atomic_fetch_add_explicit(kernelContext_3->accum_0+(_S11 + 2U), uint(color_0.z * scaled_0), memory_order_relaxed);
    uint _S15 = atomic_fetch_add_explicit(kernelContext_3->accum_0+(_S11 + 3U), weight_0, memory_order_relaxed);
    return;
}


#line 311
void blend_line_0(uint first_0, uint stride_0, uint len_0, uint offset_0, int start_1, int end_1, KernelContext_0 thread* kernelContext_4)
{

#line 311
    bool _S16;

    if(start_1 == int(0))
    {

#line 313
        _S16 = true;

#line 313
    }
    else
    {

#line 313
        _S16 = end_1 == int(0);

#line 313
    }

#line 313
    if(_S16)
    {
        return;
    }
    float _S17 = 1.0f / float(len_0);
    bool _S18 = start_1 == end_1;

#line 318
    uint i_0 = 0U;
    for(;;)
    {

#line 319
        if(i_0 < len_0)
        {
        }
        else
        {

#line 319
            break;
        }

#line 326
        float near_0 = boundary_height_0(float(i_0) * _S17, start_1, end_1, _S18);
        uint _S19 = i_0 + 1U;

#line 327
        float far_0 = boundary_height_0(float(_S19) * _S17, start_1, end_1, _S18);

#line 334
        if(near_0 <= 0.0f)
        {

#line 334
            _S16 = far_0 <= 0.0f;

#line 334
        }
        else
        {

#line 334
            _S16 = false;

#line 334
        }

#line 334
        float below_1;

#line 334
        float above_1;

#line 334
        if(_S16)
        {

#line 334
            below_1 = -0.5f * (near_0 + far_0);

#line 334
            above_1 = 0.0f;

#line 334
        }
        else
        {

#line 334
            bool _S20;

#line 339
            if(near_0 >= 0.0f)
            {

#line 339
                _S20 = far_0 >= 0.0f;

#line 339
            }
            else
            {

#line 339
                _S20 = false;

#line 339
            }

#line 339
            if(_S20)
            {

                float _S21 = 0.5f * (near_0 + far_0);

#line 342
                below_1 = 0.0f;

#line 342
                above_1 = _S21;

#line 339
            }
            else
            {

#line 346
                float crossing_0 = near_0 / (near_0 - far_0);
                float _S22 = 0.5f * abs(min(near_0, far_0));

#line 347
                if(near_0 < 0.0f)
                {

#line 347
                    below_1 = crossing_0;

#line 347
                }
                else
                {

#line 347
                    below_1 = 1.0f - crossing_0;

#line 347
                }

#line 347
                float _S23 = _S22 * below_1;
                float _S24 = 0.5f * max(near_0, far_0);

#line 348
                if(near_0 > 0.0f)
                {

#line 348
                    above_1 = crossing_0;

#line 348
                }
                else
                {

#line 348
                    above_1 = 1.0f - crossing_0;

#line 348
                }

#line 348
                float _S25 = _S24 * above_1;

#line 348
                below_1 = _S23;

#line 348
                above_1 = _S25;

#line 339
            }

#line 334
        }

#line 351
        uint pixel_0 = first_0 + i_0 * stride_0;

#line 360
        uint _S26 = pixel_0 - offset_0;

#line 360
        accumulate_0(_S26, pixel_0, below_1, kernelContext_4);

#line 360
        accumulate_0(pixel_0, _S26, above_1, kernelContext_4);

#line 319
        i_0 = _S19;

#line 319
    }

#line 363
    return;
}


#line 206
int vertical_turn_0(uint column_1, uint row_1, KernelContext_0 thread* kernelContext_5)
{

#line 206
    uint _S27 = edge_at_1(column_1, row_1, kernelContext_5);

    bool right_0 = (_S27 & 2U) != 0U;

#line 208
    bool left_0;
    if(column_1 > 0U)
    {

#line 209
        uint _S28 = edge_at_1(column_1 - 1U, row_1, kernelContext_5);

#line 209
        left_0 = (_S28 & 2U) != 0U;

#line 209
    }
    else
    {

#line 209
        left_0 = false;

#line 209
    }

#line 209
    bool _S29;
    if(left_0)
    {

#line 210
        _S29 = !right_0;

#line 210
    }
    else
    {

#line 210
        _S29 = false;

#line 210
    }

#line 210
    if(_S29)
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


#line 375
[[kernel]] void shapesMain(uint3 thread_0 [[thread_position_in_grid]], Cmaa2Params_0 constant* params_1 [[buffer(0)]], uint device* edges_1 [[buffer(1)]], texture2d<float, access::sample> source_1 [[texture(0)]], atomic<uint> device* accum_1 [[buffer(2)]])
{

#line 375
    bool _S30;

#line 375
    bool _S31;

#line 375
    thread KernelContext_0 kernelContext_6;

#line 375
    (&kernelContext_6)->params_0 = params_1;

#line 375
    (&kernelContext_6)->edges_0 = edges_1;

#line 375
    (&kernelContext_6)->source_0 = source_1;

#line 375
    (&kernelContext_6)->accum_0 = accum_1;

    uint index_0 = thread_0.x;
    uint width_1 = params_1->viewport_x_0;

    if(index_0 >= (params_1->viewport_x_0 * params_1->viewport_y_0))
    {
        return;
    }

    uint device* _S32 = (&kernelContext_6)->edges_0+index_0;

#line 385
    uint own_0 = *_S32;
    if((*_S32) == 0U)
    {
        return;
    }

    uint x_2 = index_0 % width_1;
    uint y_2 = index_0 / width_1;

#line 392
    bool _S33;



    if((own_0 & 2U) != 0U)
    {

#line 396
        uint _S34 = edge_at_0(x_2 - 1U, y_2, &kernelContext_6);

#line 396
        _S33 = (_S34 & 2U) == 0U;

#line 396
    }
    else
    {

#line 396
        _S33 = false;

#line 396
    }

#line 396
    uint len_1;

#line 396
    if(_S33)
    {

#line 396
        len_1 = 1U;


        for(;;)
        {

#line 399
            bool _S35 = len_1 <= 64U;

#line 399
            _S30 = _S35;

#line 399
            if(_S35)
            {

#line 399
                uint _S36 = edge_at_0(x_2 + len_1, y_2, &kernelContext_6);

#line 399
                _S33 = (_S36 & 2U) != 0U;

#line 399
            }
            else
            {

#line 399
                _S33 = false;

#line 399
            }

#line 399
            if(_S33)
            {
            }
            else
            {

#line 399
                break;
            }

#line 399
            len_1 = len_1 + 1U;

#line 399
        }



        if(_S30)
        {

#line 403
            int _S37 = horizontal_turn_0(x_2, y_2, &kernelContext_6);

#line 403
            int _S38 = horizontal_turn_0(x_2 + len_1, y_2, &kernelContext_6);

#line 403
            blend_line_0(index_0, 1U, len_1, width_1, _S37, _S38, &kernelContext_6);

#line 403
        }

#line 396
    }

#line 411
    if((own_0 & 1U) != 0U)
    {

#line 411
        uint _S39 = edge_at_0(x_2, y_2 - 1U, &kernelContext_6);

#line 411
        _S33 = (_S39 & 1U) == 0U;

#line 411
    }
    else
    {

#line 411
        _S33 = false;

#line 411
    }

#line 411
    if(_S33)
    {

#line 411
        len_1 = 1U;


        for(;;)
        {

#line 414
            bool _S40 = len_1 <= 64U;

#line 414
            _S31 = _S40;

#line 414
            if(_S40)
            {

#line 414
                uint _S41 = edge_at_0(x_2, y_2 + len_1, &kernelContext_6);

#line 414
                _S33 = (_S41 & 1U) != 0U;

#line 414
            }
            else
            {

#line 414
                _S33 = false;

#line 414
            }

#line 414
            if(_S33)
            {
            }
            else
            {

#line 414
                break;
            }

#line 414
            len_1 = len_1 + 1U;

#line 414
        }



        if(_S31)
        {

#line 418
            int _S42 = vertical_turn_0(x_2, y_2, &kernelContext_6);

#line 418
            int _S43 = vertical_turn_0(x_2, y_2 + len_1, &kernelContext_6);

#line 418
            blend_line_0(index_0, width_1, len_1, 1U, _S42, _S43, &kernelContext_6);

#line 418
        }

#line 411
    }

#line 423
    return;
}

