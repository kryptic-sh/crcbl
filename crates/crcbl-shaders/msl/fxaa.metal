#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 126 "shaders/fxaa.slang"
constant array<float, int(12)> SEARCH_STEP_0 = { 1.0f, 1.0f, 1.0f, 1.0f, 1.0f, 1.5f, 2.0f, 2.0f, 2.0f, 2.0f, 4.0f, 8.0f };

#line 72
struct FxaaParams_0
{
    float2 inv_source_0;
    float edge_threshold_0;
    float edge_threshold_min_0;
    float subpixel_0;
};


#line 1084 "core"
struct KernelContext_0
{
    FxaaParams_0 constant* params_0;
    texture2d<float, access::sample> source_0;
    sampler sourceSampler_0;
};


#line 153 "shaders/fxaa.slang"
float3 tap_0(float2 uv_0, float2 offset_0, KernelContext_0 thread* kernelContext_0)
{
    return ((kernelContext_0->source_0).sample((kernelContext_0->sourceSampler_0), (uv_0 + offset_0 * kernelContext_0->params_0->inv_source_0), level((0.0f)))).xyz;
}


#line 153
float3 tap_1(float2 uv_1, float2 offset_1, KernelContext_0 thread* kernelContext_1)
{
    return ((kernelContext_1->source_0).sample((kernelContext_1->sourceSampler_0), (uv_1 + offset_1 * kernelContext_1->params_0->inv_source_0), level((0.0f)))).xyz;
}


#line 134
float luma_of_0(float3 color_0)
{
    return sqrt(dot(color_0, float3(0.29899999499320984f, 0.58700001239776611f, 0.11400000005960464f)));
}


#line 159
float luma_at_0(float2 uv_2, float2 offset_2, KernelContext_0 thread* kernelContext_2)
{

#line 159
    float3 _S1 = tap_1(uv_2, offset_2, kernelContext_2);

    return luma_of_0(_S1);
}


#line 161
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 161
struct pixelInput_0
{
    float2 uv_3 [[user(TEXCOORD)]];
};


#line 176
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], FxaaParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> source_1 [[texture(0)]], sampler sourceSampler_1 [[sampler(0)]])
{

#line 176
    thread KernelContext_0 kernelContext_3;

#line 176
    (&kernelContext_3)->params_0 = params_1;

#line 176
    (&kernelContext_3)->source_0 = source_1;

#line 176
    (&kernelContext_3)->sourceSampler_0 = sourceSampler_1;

#line 176
    float3 _S3 = tap_0(_S2.uv_3, float2(0.0f, 0.0f), &kernelContext_3);

#line 183
    float luma_c_0 = luma_of_0(_S3);

#line 183
    float _S4 = luma_at_0(_S2.uv_3, float2(0.0f, -1.0f), &kernelContext_3);

#line 183
    float _S5 = luma_at_0(_S2.uv_3, float2(0.0f, 1.0f), &kernelContext_3);

#line 183
    float _S6 = luma_at_0(_S2.uv_3, float2(-1.0f, 0.0f), &kernelContext_3);

#line 183
    float _S7 = luma_at_0(_S2.uv_3, float2(1.0f, 0.0f), &kernelContext_3);

#line 190
    float _S8 = max(luma_c_0, max(max(_S4, _S5), max(_S6, _S7)));
    float range_0 = _S8 - min(luma_c_0, min(min(_S4, _S5), min(_S6, _S7)));



    if(range_0 < (max((&kernelContext_3)->params_0->edge_threshold_min_0, _S8 * (&kernelContext_3)->params_0->edge_threshold_0)))
    {

#line 195
        pixelOutput_0 _S9 = { float4(_S3, 1.0f) };

        return _S9;
    }

#line 197
    float _S10 = luma_at_0(_S2.uv_3, float2(-1.0f, -1.0f), &kernelContext_3);

#line 197
    float _S11 = luma_at_0(_S2.uv_3, float2(1.0f, -1.0f), &kernelContext_3);

#line 197
    float _S12 = luma_at_0(_S2.uv_3, float2(-1.0f, 1.0f), &kernelContext_3);

#line 197
    float _S13 = luma_at_0(_S2.uv_3, float2(1.0f, 1.0f), &kernelContext_3);

#line 206
    float luma_ns_0 = _S4 + _S5;
    float luma_we_0 = _S6 + _S7;
    float luma_wcorners_0 = _S10 + _S12;
    float luma_ecorners_0 = _S11 + _S13;

#line 217
    float _S14 = -2.0f * luma_c_0;

#line 222
    bool horizontal_0 = (abs(-2.0f * _S6 + luma_wcorners_0) + abs(_S14 + luma_ns_0) * 2.0f + abs(-2.0f * _S7 + luma_ecorners_0)) >= (abs(-2.0f * _S4 + (_S10 + _S11)) + abs(_S14 + luma_we_0) * 2.0f + abs(-2.0f * _S5 + (_S12 + _S13)));

#line 222
    float luma_1_0;


    if(horizontal_0)
    {

#line 225
        luma_1_0 = _S4;

#line 225
    }
    else
    {

#line 225
        luma_1_0 = _S6;

#line 225
    }

#line 225
    float luma_2_0;
    if(horizontal_0)
    {

#line 226
        luma_2_0 = _S5;

#line 226
    }
    else
    {

#line 226
        luma_2_0 = _S7;

#line 226
    }


    float _S15 = abs(luma_1_0 - luma_c_0);

#line 229
    float _S16 = abs(luma_2_0 - luma_c_0);

#line 229
    bool steeper_1_0 = _S15 >= _S16;
    float gradient_scaled_0 = 0.25f * max(_S15, _S16);

#line 230
    float texel_0;

#line 235
    if(horizontal_0)
    {

#line 235
        texel_0 = (&kernelContext_3)->params_0->inv_source_0.y;

#line 235
    }
    else
    {

#line 235
        texel_0 = (&kernelContext_3)->params_0->inv_source_0.x;

#line 235
    }

#line 235
    float luma_local_0;
    if(steeper_1_0)
    {

#line 236
        luma_local_0 = 0.5f * (luma_1_0 + luma_c_0);

#line 236
    }
    else
    {

#line 236
        luma_local_0 = 0.5f * (luma_2_0 + luma_c_0);

#line 236
    }

#line 236
    float step_length_0;
    if(steeper_1_0)
    {

#line 237
        step_length_0 = - texel_0;

#line 237
    }
    else
    {

#line 237
        step_length_0 = texel_0;

#line 237
    }

    thread float2 edge_uv_0 = _S2.uv_3;
    if(horizontal_0)
    {
        edge_uv_0.y = edge_uv_0.y + step_length_0 * 0.5f;

#line 240
    }
    else
    {



        edge_uv_0.x = edge_uv_0.x + step_length_0 * 0.5f;

#line 240
    }

#line 240
    float2 along_0;

#line 251
    if(horizontal_0)
    {

#line 251
        along_0 = float2((&kernelContext_3)->params_0->inv_source_0.x, 0.0f);

#line 251
    }
    else
    {

#line 251
        along_0 = float2(0.0f, (&kernelContext_3)->params_0->inv_source_0.y);

#line 251
    }


    float2 uv_neg_0 = edge_uv_0 - along_0;
    float2 uv_pos_0 = edge_uv_0 + along_0;
    float delta_neg_0 = luma_of_0((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (uv_neg_0), level((0.0f)))).xyz) - luma_local_0;
    float delta_pos_0 = luma_of_0((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (uv_pos_0), level((0.0f)))).xyz) - luma_local_0;
    bool _S17 = (abs(delta_neg_0)) >= gradient_scaled_0;
    bool _S18 = (abs(delta_pos_0)) >= gradient_scaled_0;
    if(horizontal_0)
    {

#line 260
        luma_1_0 = _S2.uv_3.x - uv_neg_0.x;

#line 260
    }
    else
    {

#line 260
        luma_1_0 = _S2.uv_3.y - uv_neg_0.y;

#line 260
    }
    if(horizontal_0)
    {

#line 261
        luma_2_0 = uv_pos_0.x - _S2.uv_3.x;

#line 261
    }
    else
    {

#line 261
        luma_2_0 = uv_pos_0.y - _S2.uv_3.y;

#line 261
    }

#line 261
    bool done_neg_0 = _S17;

#line 261
    bool done_pos_0 = _S18;

#line 261
    float distance_neg_0 = luma_1_0;

#line 261
    float distance_pos_0 = luma_2_0;

#line 261
    float delta_neg_1 = delta_neg_0;

#line 261
    float delta_pos_1 = delta_pos_0;

#line 261
    int i_0 = int(0);

#line 261
    float2 uv_neg_1 = uv_neg_0;

#line 261
    float2 uv_pos_1 = uv_pos_0;



    for(;;)
    {

#line 265
        if(i_0 < int(12))
        {
        }
        else
        {

#line 265
            break;
        }

#line 265
        int _S19 = i_0;


        if(!done_neg_0)
        {
            float2 uv_neg_2 = uv_neg_1 - along_0 * float2(SEARCH_STEP_0[_S19]) ;
            float delta_neg_2 = luma_of_0((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (uv_neg_2), level((0.0f)))).xyz) - luma_local_0;
            bool _S20 = (abs(delta_neg_2)) >= gradient_scaled_0;
            if(horizontal_0)
            {

#line 273
                luma_1_0 = _S2.uv_3.x - uv_neg_2.x;

#line 273
            }
            else
            {

#line 273
                luma_1_0 = _S2.uv_3.y - uv_neg_2.y;

#line 273
            }

#line 273
            done_neg_0 = _S20;

#line 273
            distance_neg_0 = luma_1_0;

#line 273
            delta_neg_1 = delta_neg_2;

#line 273
            uv_neg_1 = uv_neg_2;

#line 268
        }

#line 275
        if(!done_pos_0)
        {
            float2 uv_pos_2 = uv_pos_1 + along_0 * float2(SEARCH_STEP_0[_S19]) ;
            float delta_pos_2 = luma_of_0((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (uv_pos_2), level((0.0f)))).xyz) - luma_local_0;
            bool _S21 = (abs(delta_pos_2)) >= gradient_scaled_0;
            if(horizontal_0)
            {

#line 280
                luma_1_0 = uv_pos_2.x - _S2.uv_3.x;

#line 280
            }
            else
            {

#line 280
                luma_1_0 = uv_pos_2.y - _S2.uv_3.y;

#line 280
            }

#line 280
            done_pos_0 = _S21;

#line 280
            distance_pos_0 = luma_1_0;

#line 280
            delta_pos_1 = delta_pos_2;

#line 280
            uv_pos_1 = uv_pos_2;

#line 275
        }

#line 265
        i_0 = i_0 + int(1);

#line 265
    }

#line 288
    float _S22 = max(0.0f, 0.5f - min(distance_neg_0, distance_pos_0) / max(distance_neg_0 + distance_pos_0, 9.99999997475242708e-07f)) * step_length_0;

#line 288
    float delta_nearer_0;

#line 294
    if(distance_neg_0 < distance_pos_0)
    {

#line 294
        delta_nearer_0 = delta_neg_1;

#line 294
    }
    else
    {

#line 294
        delta_nearer_0 = delta_pos_1;

#line 294
    }

#line 294
    float offset_3;
    if(((luma_c_0 - luma_local_0) < 0.0f) == (delta_nearer_0 < 0.0f))
    {

#line 295
        offset_3 = 0.0f;

#line 295
    }
    else
    {

#line 295
        offset_3 = _S22;

#line 295
    }

#line 303
    float subpixel_ratio_0 = saturate(abs((2.0f * (luma_ns_0 + luma_we_0) + luma_wcorners_0 + luma_ecorners_0) * 0.0833333358168602f - luma_c_0) / max(range_0, 9.99999997475242708e-07f));
    float subpixel_weight_0 = (-2.0f * subpixel_ratio_0 + 3.0f) * subpixel_ratio_0 * subpixel_ratio_0;
    float subpixel_offset_0 = subpixel_weight_0 * subpixel_weight_0 * (&kernelContext_3)->params_0->subpixel_0 * step_length_0;

#line 305
    float final_offset_0;



    if((abs(subpixel_offset_0)) > (abs(offset_3)))
    {

#line 309
        final_offset_0 = subpixel_offset_0;

#line 309
    }
    else
    {

#line 309
        final_offset_0 = offset_3;

#line 309
    }

    thread float2 result_uv_0 = _S2.uv_3;
    if(horizontal_0)
    {
        result_uv_0.y = result_uv_0.y + final_offset_0;

#line 312
    }
    else
    {



        result_uv_0.x = result_uv_0.x + final_offset_0;

#line 312
    }

#line 312
    pixelOutput_0 _S23 = { float4((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (result_uv_0), level((0.0f)))).xyz, 1.0f) };

#line 321
    return _S23;
}


#line 321
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_4 [[user(TEXCOORD)]];
};


#line 110
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_5;
};


#line 110
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], FxaaParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> source_2 [[texture(0)]], sampler sourceSampler_2 [[sampler(0)]])
{

#line 110
    thread KernelContext_0 kernelContext_4;

#line 110
    (&kernelContext_4)->params_0 = params_2;

#line 110
    (&kernelContext_4)->source_0 = source_2;

#line 110
    (&kernelContext_4)->sourceSampler_0 = sourceSampler_2;

#line 167
    thread FullscreenOutput_0 output_1;


    float2 _S24 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 170
    (&output_1)->uv_5 = _S24;
    (&output_1)->position_2 = float4(_S24 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 171
    thread vertexMain_Result_0 _S25;

#line 171
    (&_S25)->position_1 = output_1.position_2;

#line 171
    (&_S25)->uv_4 = output_1.uv_5;

#line 171
    return _S25;
}

