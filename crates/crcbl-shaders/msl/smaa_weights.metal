#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 66 "shaders/smaa_weights.slang"
struct SmaaParams_0
{
    float2 inv_source_0;
    float2 source_size_0;
};


#line 1084 "core"
struct KernelContext_0
{
    SmaaParams_0 constant* params_0;
    texture2d<float, access::sample> edges_0;
    sampler tableSampler_0;
    texture2d<float, access::sample> area_0;
    texture2d<float, access::sample> search_0;
};


#line 139 "shaders/smaa_weights.slang"
float4 sample_edges_0(float2 uv_0, KernelContext_0 thread* kernelContext_0)
{
    return ((kernelContext_0->edges_0).sample((kernelContext_0->tableSampler_0), (uv_0), level((0.0f))));
}


#line 176
float2 search_diag1_0(float2 uv_1, float2 dir_0, float2 thread* e_0, KernelContext_0 thread* kernelContext_1)
{
    float3 _S1 = float3(uv_1, -1.0f);

    *e_0 = float2(0.0f, 0.0f);

#line 180
    float weight_0 = 1.0f;

#line 180
    int i_0 = int(0);

#line 180
    float3 coord_0 = _S1;
    for(;;)
    {

#line 181
        if(i_0 < int(8))
        {
        }
        else
        {

#line 181
            break;
        }

#line 181
        bool _S2;

        if((coord_0.z) < 7.0f)
        {

#line 183
            _S2 = weight_0 > 0.89999997615814209f;

#line 183
        }
        else
        {

#line 183
            _S2 = false;

#line 183
        }

#line 183
        if(_S2)
        {
            float3 coord_1 = coord_0 + float3(dir_0 * kernelContext_1->params_0->inv_source_0, 1.0f);

#line 185
            float4 _S3 = sample_edges_0(coord_1.xy, kernelContext_1);
            float2 _S4 = _S3.xy;

#line 186
            *e_0 = _S4;

#line 186
            weight_0 = dot(_S4, float2(0.5f, 0.5f));

#line 186
            coord_0 = coord_1;

#line 183
        }

#line 181
        i_0 = i_0 + int(1);

#line 181
    }

#line 190
    return float2(coord_0.z, weight_0);
}


#line 150
float4 sample_edges_at_0(float2 uv_2, float2 offset_0, KernelContext_0 thread* kernelContext_2)
{
    return ((kernelContext_2->edges_0).sample((kernelContext_2->tableSampler_0), (uv_2 + offset_0 * kernelContext_2->params_0->inv_source_0), level((0.0f))));
}


#line 167
float4 decode_diag_bilinear4_0(float4 e_1)
{

#line 167
    thread float4 _S5 = e_1;

    float _S6 = e_1.x;

#line 169
    _S5.x = _S6 * abs(5.0f * _S6 - 3.75f);
    _S5.z = _S5.z * abs(5.0f * _S5.z - 3.75f);
    return round(_S5);
}


#line 220
float2 area_diag_0(float2 dist_0, float2 e_2, KernelContext_0 thread* kernelContext_3)
{
    thread float2 texcoord_0 = float2(0.00625000009313226f, 0.01250000018626451f) * (float2(20.0f, 20.0f) * e_2 + dist_0) + float2(0.5f)  * float2(0.00625000009313226f, 0.01250000018626451f);


    texcoord_0.x = texcoord_0.x + 0.5f;
    return ((kernelContext_3->area_0).sample((kernelContext_3->tableSampler_0), (texcoord_0), level((0.0f)))).xy;
}


#line 159
float2 decode_diag_bilinear_0(float2 e_3)
{

#line 159
    thread float2 _S7 = e_3;

    float _S8 = e_3.x;

#line 161
    _S7.x = _S8 * abs(5.0f * _S8 - 3.75f);
    return round(_S7);
}


#line 196
float2 search_diag2_0(float2 uv_3, float2 dir_1, float2 thread* e_4, KernelContext_0 thread* kernelContext_4)
{
    thread float3 coord_2 = float3(uv_3, -1.0f);
    coord_2.x = coord_2.x + 0.25f * kernelContext_4->params_0->inv_source_0.x;

    *e_4 = float2(0.0f, 0.0f);

#line 201
    float weight_1 = 1.0f;

#line 201
    int i_1 = int(0);
    for(;;)
    {

#line 202
        if(i_1 < int(8))
        {
        }
        else
        {

#line 202
            break;
        }

#line 202
        bool _S9;

        if((coord_2.z) < 7.0f)
        {

#line 204
            _S9 = weight_1 > 0.89999997615814209f;

#line 204
        }
        else
        {

#line 204
            _S9 = false;

#line 204
        }

#line 204
        if(_S9)
        {
            float3 _S10 = coord_2 + float3(dir_1 * kernelContext_4->params_0->inv_source_0, 1.0f);

#line 206
            coord_2 = _S10;

#line 206
            float4 _S11 = sample_edges_0(_S10.xy, kernelContext_4);
            float2 _S12 = decode_diag_bilinear_0(_S11.xy);

#line 207
            *e_4 = _S12;

#line 207
            weight_1 = dot(_S12, float2(0.5f, 0.5f));

#line 204
        }

#line 202
        i_1 = i_1 + int(1);

#line 202
    }

#line 211
    return float2(coord_2.z, weight_1);
}


#line 235
float2 calculate_diag_weights_0(float2 uv_4, float2 e_5, KernelContext_0 thread* kernelContext_5)
{
    float2 weights_0 = float2(0.0f, 0.0f);



    thread float4 d_0;
    thread float2 end_0;
    if((e_5.x) > 0.0f)
    {

#line 243
        float2 _S13 = search_diag1_0(uv_4, float2(-1.0f, 1.0f), &end_0, kernelContext_5);


        float _S14 = _S13.x;
        d_0.z = _S13.y;
        d_0.x = _S14 + float((end_0.y) > 0.89999997615814209f);

#line 243
    }
    else
    {

#line 252
        d_0.x = 0.0f;
        d_0.z = 0.0f;

#line 243
    }

#line 243
    float2 _S15 = search_diag1_0(uv_4, float2(1.0f, -1.0f), &end_0, kernelContext_5);

#line 256
    d_0.y = _S15.x;
    d_0.w = _S15.y;

#line 257
    float2 weights_1;



    if((d_0.x + d_0.y) > 2.0f)
    {


        float4 coords_0 = float4(- d_0.x + 0.25f, d_0.x, d_0.y, - d_0.y - 0.25f) * float4(kernelContext_5->params_0->inv_source_0, kernelContext_5->params_0->inv_source_0) + float4(uv_4, uv_4);

#line 265
        float4 _S16 = sample_edges_at_0(coords_0.xy, float2(-1.0f, 0.0f), kernelContext_5);
        thread float4 fetched_0;
        fetched_0.xy = _S16.xy;

#line 267
        float4 _S17 = sample_edges_at_0(coords_0.zw, float2(1.0f, 0.0f), kernelContext_5);
        fetched_0.zw = _S17.xy;



        float4 decoded_0 = decode_diag_bilinear4_0(fetched_0);


        thread float2 crossing_0 = float2(2.0f, 2.0f) * float2(decoded_0.y, decoded_0.w) + float2(decoded_0.x, decoded_0.z);



        if((d_0.z) >= 0.89999997615814209f)
        {
            crossing_0.x = 0.0f;

#line 279
        }



        if((d_0.w) >= 0.89999997615814209f)
        {
            crossing_0.y = 0.0f;

#line 283
        }

#line 283
        float2 _S18 = area_diag_0(d_0.xy, crossing_0, kernelContext_5);

#line 283
        weights_1 = _S18;

#line 261
    }
    else
    {

#line 261
        weights_1 = weights_0;

#line 261
    }

#line 261
    float2 _S19 = search_diag2_0(uv_4, float2(-1.0f, -1.0f), &end_0, kernelContext_5);

#line 292
    d_0.x = _S19.x;
    d_0.z = _S19.y;
    float2 _S20 = float2(1.0f, 0.0f);

#line 294
    float4 _S21 = sample_edges_at_0(uv_4, _S20, kernelContext_5);

#line 294
    if((_S21.x) > 0.0f)
    {

#line 294
        float2 _S22 = search_diag2_0(uv_4, float2(1.0f, 1.0f), &end_0, kernelContext_5);


        float _S23 = _S22.x;
        d_0.w = _S22.y;
        d_0.y = _S23 + float((end_0.y) > 0.89999997615814209f);

#line 294
    }
    else
    {

#line 303
        d_0.y = 0.0f;
        d_0.w = 0.0f;

#line 294
    }

#line 307
    if((d_0.x + d_0.y) > 2.0f)
    {


        float4 coords_1 = float4(- d_0.x, - d_0.x, d_0.y, d_0.y) * float4(kernelContext_5->params_0->inv_source_0, kernelContext_5->params_0->inv_source_0) + float4(uv_4, uv_4);
        thread float4 c_0;
        float2 _S24 = coords_1.xy;

#line 313
        float4 _S25 = sample_edges_at_0(_S24, float2(-1.0f, 0.0f), kernelContext_5);

#line 313
        c_0.x = _S25.y;

#line 313
        float4 _S26 = sample_edges_at_0(_S24, float2(0.0f, -1.0f), kernelContext_5);
        c_0.y = _S26.x;

#line 314
        float4 _S27 = sample_edges_at_0(coords_1.zw, _S20, kernelContext_5);
        float2 far_0 = _S27.xy;

        c_0.z = far_0.y;
        c_0.w = far_0.x;

        thread float2 crossing_1 = float2(2.0f, 2.0f) * c_0.xz + c_0.yw;
        if((d_0.z) >= 0.89999997615814209f)
        {
            crossing_1.x = 0.0f;

#line 321
        }



        if((d_0.w) >= 0.89999997615814209f)
        {
            crossing_1.y = 0.0f;

#line 325
        }

#line 325
        float2 _S28 = area_diag_0(d_0.xy, crossing_1, kernelContext_5);

#line 325
        weights_1 = weights_1 + float2(_S28.y, _S28.x);

#line 307
    }

#line 334
    return weights_1;
}


#line 344
float search_length_0(float2 e_6, float offset_1, KernelContext_0 thread* kernelContext_6)
{

#line 344
    float2 _S29 = float2(1.0f) ;

#line 360
    return ((kernelContext_6->search_0).sample((kernelContext_6->tableSampler_0), ((float2(66.0f, 33.0f) * float2(0.5f, -1.0f) + float2(-1.0f, 1.0f)) * (_S29 / float2(64.0f, 16.0f)) * e_6 + (float2(66.0f, 33.0f) * float2(offset_1, 1.0f) + float2(0.5f, -0.5f)) * (_S29 / float2(64.0f, 16.0f))), level((0.0f)))).x;
}




float search_x_left_0(float2 uv_5, KernelContext_0 thread* kernelContext_7)
{

#line 366
    float2 e_7 = float2(0.0f, 1.0f);

#line 366
    int i_2 = int(0);

#line 366
    float2 coord_3 = uv_5;



    for(;;)
    {

#line 370
        if(i_2 < int(16))
        {
        }
        else
        {

#line 370
            break;
        }

#line 370
        bool _S30;



        if((e_7.y) > 0.82810002565383911f)
        {

#line 374
            _S30 = (e_7.x) == 0.0f;

#line 374
        }
        else
        {

#line 374
            _S30 = false;

#line 374
        }

#line 374
        if(_S30)
        {

#line 374
            float4 _S31 = sample_edges_0(coord_3, kernelContext_7);


            float2 coord_4 = coord_3 - float2(2.0f, 0.0f) * kernelContext_7->params_0->inv_source_0;

#line 377
            e_7 = _S31.xy;

#line 377
            coord_3 = coord_4;

#line 374
        }

#line 370
        i_2 = i_2 + int(1);

#line 370
    }

#line 370
    float _S32 = search_length_0(e_7, 0.0f, kernelContext_7);

#line 381
    return kernelContext_7->params_0->inv_source_0.x * (-2.0078740119934082f * _S32 + 3.25f) + coord_3.x;
}


float search_x_right_0(float2 uv_6, KernelContext_0 thread* kernelContext_8)
{

#line 385
    float2 e_8 = float2(0.0f, 1.0f);

#line 385
    int i_3 = int(0);

#line 385
    float2 coord_5 = uv_6;



    for(;;)
    {

#line 389
        if(i_3 < int(16))
        {
        }
        else
        {

#line 389
            break;
        }

#line 389
        bool _S33;

        if((e_8.y) > 0.82810002565383911f)
        {

#line 391
            _S33 = (e_8.x) == 0.0f;

#line 391
        }
        else
        {

#line 391
            _S33 = false;

#line 391
        }

#line 391
        if(_S33)
        {

#line 391
            float4 _S34 = sample_edges_0(coord_5, kernelContext_8);


            float2 coord_6 = coord_5 + float2(2.0f, 0.0f) * kernelContext_8->params_0->inv_source_0;

#line 394
            e_8 = _S34.xy;

#line 394
            coord_5 = coord_6;

#line 391
        }

#line 389
        i_3 = i_3 + int(1);

#line 389
    }

#line 389
    float _S35 = search_length_0(e_8, 0.5f, kernelContext_8);

#line 398
    return - kernelContext_8->params_0->inv_source_0.x * (-2.0078740119934082f * _S35 + 3.25f) + coord_5.x;
}


#line 442
float2 area_ortho_0(float2 dist_1, float e1_0, float e2_0, KernelContext_0 thread* kernelContext_9)
{

#line 448
    return ((kernelContext_9->area_0).sample((kernelContext_9->tableSampler_0), (float2(0.00625000009313226f, 0.01250000018626451f) * (float2(16.0f, 16.0f) * round(float2(4.0f)  * float2(e1_0, e2_0)) + dist_1) + float2(0.5f)  * float2(0.00625000009313226f, 0.01250000018626451f)), level((0.0f)))).xy;
}



float2 horizontal_corner_factor_0(float4 texcoord_1, float2 d_1, KernelContext_0 thread* kernelContext_10)
{
    float2 left_right_0 = step(d_1.xy, d_1.yx);


    float2 rounding_0 = float2(0.75f)  * left_right_0 / float2((left_right_0.x + left_right_0.y)) ;

    float2 _S36 = float2(1.0f, 1.0f);

#line 460
    thread float2 factor_0 = _S36;
    float _S37 = rounding_0.x;

#line 461
    float2 _S38 = texcoord_1.xy;

#line 461
    float4 _S39 = sample_edges_at_0(_S38, float2(0.0f, 1.0f), kernelContext_10);

#line 461
    float _S40 = factor_0.x - _S37 * _S39.x;
    float _S41 = rounding_0.y;

#line 462
    float2 _S42 = texcoord_1.zw;

#line 462
    float4 _S43 = sample_edges_at_0(_S42, _S36, kernelContext_10);

#line 462
    factor_0.x = _S40 - _S41 * _S43.x;

#line 462
    float4 _S44 = sample_edges_at_0(_S38, float2(0.0f, -2.0f), kernelContext_10);
    float _S45 = factor_0.y - _S37 * _S44.x;

#line 463
    float4 _S46 = sample_edges_at_0(_S42, float2(1.0f, -2.0f), kernelContext_10);
    factor_0.y = _S45 - _S41 * _S46.x;
    return saturate(factor_0);
}


#line 402
float search_y_up_0(float2 uv_7, KernelContext_0 thread* kernelContext_11)
{

#line 402
    float2 e_9 = float2(1.0f, 0.0f);

#line 402
    int i_4 = int(0);

#line 402
    float2 coord_7 = uv_7;



    for(;;)
    {

#line 406
        if(i_4 < int(16))
        {
        }
        else
        {

#line 406
            break;
        }

#line 406
        bool _S47;

        if((e_9.x) > 0.82810002565383911f)
        {

#line 408
            _S47 = (e_9.y) == 0.0f;

#line 408
        }
        else
        {

#line 408
            _S47 = false;

#line 408
        }

#line 408
        if(_S47)
        {

#line 408
            float4 _S48 = sample_edges_0(coord_7, kernelContext_11);


            float2 coord_8 = coord_7 - float2(0.0f, 2.0f) * kernelContext_11->params_0->inv_source_0;

#line 411
            e_9 = _S48.xy;

#line 411
            coord_7 = coord_8;

#line 408
        }

#line 406
        i_4 = i_4 + int(1);

#line 406
    }

#line 406
    float _S49 = search_length_0(float2(e_9.y, e_9.x), 0.0f, kernelContext_11);

#line 415
    return kernelContext_11->params_0->inv_source_0.y * (-2.0078740119934082f * _S49 + 3.25f) + coord_7.y;
}


float search_y_down_0(float2 uv_8, KernelContext_0 thread* kernelContext_12)
{

#line 419
    float2 e_10 = float2(1.0f, 0.0f);

#line 419
    int i_5 = int(0);

#line 419
    float2 coord_9 = uv_8;



    for(;;)
    {

#line 423
        if(i_5 < int(16))
        {
        }
        else
        {

#line 423
            break;
        }

#line 423
        bool _S50;

        if((e_10.x) > 0.82810002565383911f)
        {

#line 425
            _S50 = (e_10.y) == 0.0f;

#line 425
        }
        else
        {

#line 425
            _S50 = false;

#line 425
        }

#line 425
        if(_S50)
        {

#line 425
            float4 _S51 = sample_edges_0(coord_9, kernelContext_12);


            float2 coord_10 = coord_9 + float2(0.0f, 2.0f) * kernelContext_12->params_0->inv_source_0;

#line 428
            e_10 = _S51.xy;

#line 428
            coord_9 = coord_10;

#line 425
        }

#line 423
        i_5 = i_5 + int(1);

#line 423
    }

#line 423
    float _S52 = search_length_0(float2(e_10.y, e_10.x), 0.5f, kernelContext_12);

#line 432
    return - kernelContext_12->params_0->inv_source_0.y * (-2.0078740119934082f * _S52 + 3.25f) + coord_9.y;
}


#line 469
float2 vertical_corner_factor_0(float4 texcoord_2, float2 d_2, KernelContext_0 thread* kernelContext_13)
{
    float2 left_right_1 = step(d_2.xy, d_2.yx);

    float2 rounding_1 = float2(0.75f)  * left_right_1 / float2((left_right_1.x + left_right_1.y)) ;

    float2 _S53 = float2(1.0f, 1.0f);

#line 475
    thread float2 factor_1 = _S53;
    float _S54 = rounding_1.x;

#line 476
    float2 _S55 = texcoord_2.xy;

#line 476
    float4 _S56 = sample_edges_at_0(_S55, float2(1.0f, 0.0f), kernelContext_13);

#line 476
    float _S57 = factor_1.x - _S54 * _S56.y;
    float _S58 = rounding_1.y;

#line 477
    float2 _S59 = texcoord_2.zw;

#line 477
    float4 _S60 = sample_edges_at_0(_S59, _S53, kernelContext_13);

#line 477
    factor_1.x = _S57 - _S58 * _S60.y;

#line 477
    float4 _S61 = sample_edges_at_0(_S55, float2(-2.0f, 0.0f), kernelContext_13);
    float _S62 = factor_1.y - _S54 * _S61.y;

#line 478
    float4 _S63 = sample_edges_at_0(_S59, float2(-2.0f, 1.0f), kernelContext_13);
    factor_1.y = _S62 - _S58 * _S63.y;
    return saturate(factor_1);
}


#line 480
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 480
struct pixelInput_0
{
    float2 uv_9 [[user(TEXCOORD)]];
};


#line 493
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S64 [[stage_in]], float4 position_0 [[position]], SmaaParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> edges_1 [[texture(0)]], sampler tableSampler_1 [[sampler(0)]], texture2d<float, access::sample> area_1 [[texture(1)]], texture2d<float, access::sample> search_1 [[texture(2)]])
{

#line 493
    thread KernelContext_0 kernelContext_14;

#line 493
    (&kernelContext_14)->params_0 = params_1;

#line 493
    (&kernelContext_14)->edges_0 = edges_1;

#line 493
    (&kernelContext_14)->tableSampler_0 = tableSampler_1;

#line 493
    (&kernelContext_14)->area_0 = area_1;

#line 493
    (&kernelContext_14)->search_0 = search_1;

#line 498
    float2 pixcoord_0 = _S64.uv_9 * params_1->source_size_0;

#line 504
    float4 _S65 = float4(params_1->inv_source_0, params_1->inv_source_0);

#line 504
    float4 _S66 = float4(_S64.uv_9, _S64.uv_9);

#line 504
    float4 offset_h_0 = float4(-0.25f, -0.125f, 1.25f, -0.125f) * _S65 + _S66;
    float4 offset_v_0 = float4(-0.125f, -0.25f, -0.125f, 1.25f) * _S65 + _S66;

    thread float4 weights_2 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 507
    float4 _S67 = sample_edges_0(_S64.uv_9, &kernelContext_14);
    float2 _S68 = _S67.xy;

#line 508
    thread float2 e_11 = _S68;

    if((_S68.y) > 0.0f)
    {

#line 510
        float2 _S69 = calculate_diag_weights_0(_S64.uv_9, e_11, &kernelContext_14);

#line 515
        weights_2.x = _S69.x;
        weights_2.y = _S69.y;

#line 521
        if((weights_2.x) == (- weights_2.y))
        {

            thread float3 coords_2;

#line 524
            float _S70 = search_x_left_0(offset_h_0.xy, &kernelContext_14);

            coords_2.x = _S70;


            coords_2.y = offset_v_0.y;

#line 523
            thread float2 d_3;

#line 530
            d_3.x = coords_2.x;

#line 530
            float4 _S71 = sample_edges_0(coords_2.xy, &kernelContext_14);
            float e1_1 = _S71.x;

#line 531
            float _S72 = search_x_right_0(offset_h_0.zw, &kernelContext_14);

            coords_2.z = _S72;
            d_3.y = coords_2.z;


            float2 _S73 = abs(round(params_1->source_size_0.xx * d_3 - pixcoord_0.xx));

#line 537
            d_3 = _S73;

            float2 sqrt_d_0 = sqrt(_S73);

#line 539
            float4 _S74 = sample_edges_at_0(float2(coords_2.z, coords_2.y), float2(1.0f, 0.0f), &kernelContext_14);

#line 539
            float2 _S75 = area_ortho_0(sqrt_d_0, e1_1, _S74.x, &kernelContext_14);

#line 545
            coords_2.y = _S64.uv_9.y;

#line 545
            float2 _S76 = horizontal_corner_factor_0(float4(coords_2.x, coords_2.y, coords_2.z, coords_2.y), _S73, &kernelContext_14);
            float2 found_0 = _S75 * _S76;

            weights_2.x = found_0.x;
            weights_2.y = found_0.y;

#line 521
        }
        else
        {

#line 554
            e_11.x = 0.0f;

#line 521
        }

#line 510
    }

#line 558
    if((e_11.x) > 0.0f)
    {

        thread float3 coords_3;

#line 561
        float _S77 = search_y_up_0(offset_v_0.xy, &kernelContext_14);

        coords_3.y = _S77;
        coords_3.x = offset_h_0.x;

#line 560
        thread float2 d_4;

#line 565
        d_4.x = coords_3.y;

#line 565
        float4 _S78 = sample_edges_0(coords_3.xy, &kernelContext_14);
        float e1_2 = _S78.y;

#line 566
        float _S79 = search_y_down_0(offset_v_0.zw, &kernelContext_14);

        coords_3.z = _S79;
        d_4.y = coords_3.z;

        float2 _S80 = abs(round(params_1->source_size_0.yy * d_4 - pixcoord_0.yy));

#line 571
        d_4 = _S80;
        float2 sqrt_d_1 = sqrt(_S80);

#line 572
        float4 _S81 = sample_edges_at_0(float2(coords_3.x, coords_3.z), float2(0.0f, 1.0f), &kernelContext_14);

#line 572
        float2 _S82 = area_ortho_0(sqrt_d_1, e1_2, _S81.y, &kernelContext_14);

#line 578
        coords_3.x = _S64.uv_9.x;

#line 578
        float2 _S83 = vertical_corner_factor_0(float4(coords_3.x, coords_3.y, coords_3.x, coords_3.z), _S80, &kernelContext_14);
        float2 found_1 = _S82 * _S83;
        weights_2.z = found_1.x;
        weights_2.w = found_1.y;

#line 558
    }

#line 558
    pixelOutput_0 _S84 = { weights_2 };

#line 584
    return _S84;
}


#line 584
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_10 [[user(TEXCOORD)]];
};


#line 97
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_11;
};


#line 97
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], SmaaParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> edges_2 [[texture(0)]], sampler tableSampler_2 [[sampler(0)]], texture2d<float, access::sample> area_2 [[texture(1)]], texture2d<float, access::sample> search_2 [[texture(2)]])
{

#line 97
    thread KernelContext_0 kernelContext_15;

#line 97
    (&kernelContext_15)->params_0 = params_2;

#line 97
    (&kernelContext_15)->edges_0 = edges_2;

#line 97
    (&kernelContext_15)->tableSampler_0 = tableSampler_2;

#line 97
    (&kernelContext_15)->area_0 = area_2;

#line 97
    (&kernelContext_15)->search_0 = search_2;

#line 486
    thread FullscreenOutput_0 output_1;
    float2 _S85 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 487
    (&output_1)->uv_11 = _S85;
    (&output_1)->position_2 = float4(_S85 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 488
    thread vertexMain_Result_0 _S86;

#line 488
    (&_S86)->position_1 = output_1.position_2;

#line 488
    (&_S86)->uv_10 = output_1.uv_11;

#line 488
    return _S86;
}

