#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 138 "shaders/fxaa.slang"
constant array<float, int(12)> SEARCH_STEP_0 = { { 1.0f, 1.0f, 1.0f, 1.0f, 1.0f, 1.5f, 2.0f, 2.0f, 2.0f, 2.0f, 4.0f, 8.0f } };

#line 84
struct FxaaParams_0
{
    float2 inv_source_0;
    float edge_threshold_0;
    float edge_threshold_min_0;
    float subpixel_0;
};


#line 117
struct KernelContext_0
{
    FxaaParams_0 constant* params_0;
    texture2d<float, access::sample> source_0;
    sampler sourceSampler_0;
};


#line 165
float3 tap_0(float2 uv_0, float2 offset_0, KernelContext_0 thread* kernelContext_0)
{
    return ((kernelContext_0->source_0).sample((kernelContext_0->sourceSampler_0), (uv_0 + offset_0 * kernelContext_0->params_0->inv_source_0), level((0.0f)))).xyz;
}


#line 165
float3 tap_1(float2 uv_1, float2 offset_1, KernelContext_0 thread* kernelContext_1)
{
    return ((kernelContext_1->source_0).sample((kernelContext_1->sourceSampler_0), (uv_1 + offset_1 * kernelContext_1->params_0->inv_source_0), level((0.0f)))).xyz;
}


#line 146
float luma_of_0(float3 color_0)
{
    return sqrt(dot(color_0, float3(0.29899999499320984f, 0.58700001239776611f, 0.11400000005960464f)));
}


#line 171
float luma_at_0(float2 uv_2, float2 offset_2, KernelContext_0 thread* kernelContext_2)
{

#line 171
    float3 _S1 = tap_1(uv_2, offset_2, kernelContext_2);

    return luma_of_0(_S1);
}


#line 173
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 173
struct pixelInput_0
{
    float2 uv_3 [[user(TEXCOORD)]];
};


#line 188
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], FxaaParams_0 constant* params_1 [[buffer(0)]], texture2d<float, access::sample> source_1 [[texture(0)]], sampler sourceSampler_1 [[sampler(0)]])
{

#line 188
    thread KernelContext_0 kernelContext_3;

#line 188
    (&kernelContext_3)->params_0 = params_1;

#line 188
    (&kernelContext_3)->source_0 = source_1;

#line 188
    (&kernelContext_3)->sourceSampler_0 = sourceSampler_1;

#line 188
    float3 _S3 = tap_0(_S2.uv_3, float2(0.0f, 0.0f), &kernelContext_3);

#line 195
    float luma_c_0 = luma_of_0(_S3);

#line 195
    float _S4 = luma_at_0(_S2.uv_3, float2(0.0f, -1.0f), &kernelContext_3);

#line 195
    float _S5 = luma_at_0(_S2.uv_3, float2(0.0f, 1.0f), &kernelContext_3);

#line 195
    float _S6 = luma_at_0(_S2.uv_3, float2(-1.0f, 0.0f), &kernelContext_3);

#line 195
    float _S7 = luma_at_0(_S2.uv_3, float2(1.0f, 0.0f), &kernelContext_3);

#line 202
    float _S8 = max(luma_c_0, max(max(_S4, _S5), max(_S6, _S7)));
    float range_0 = _S8 - min(luma_c_0, min(min(_S4, _S5), min(_S6, _S7)));



    if(range_0 < (max((&kernelContext_3)->params_0->edge_threshold_min_0, _S8 * (&kernelContext_3)->params_0->edge_threshold_0)))
    {

#line 207
        pixelOutput_0 _S9 = { float4(_S3, 1.0f) };

        return _S9;
    }

#line 209
    float _S10 = luma_at_0(_S2.uv_3, float2(-1.0f, -1.0f), &kernelContext_3);

#line 209
    float _S11 = luma_at_0(_S2.uv_3, float2(1.0f, -1.0f), &kernelContext_3);

#line 209
    float _S12 = luma_at_0(_S2.uv_3, float2(-1.0f, 1.0f), &kernelContext_3);

#line 209
    float _S13 = luma_at_0(_S2.uv_3, float2(1.0f, 1.0f), &kernelContext_3);

#line 218
    float luma_ns_0 = _S4 + _S5;
    float luma_we_0 = _S6 + _S7;
    float luma_wcorners_0 = _S10 + _S12;
    float luma_ecorners_0 = _S11 + _S13;

#line 229
    float _S14 = -2.0f * luma_c_0;

#line 234
    bool horizontal_0 = (abs(-2.0f * _S6 + luma_wcorners_0) + abs(_S14 + luma_ns_0) * 2.0f + abs(-2.0f * _S7 + luma_ecorners_0)) >= (abs(-2.0f * _S4 + (_S10 + _S11)) + abs(_S14 + luma_we_0) * 2.0f + abs(-2.0f * _S5 + (_S12 + _S13)));

#line 234
    float luma_1_0;


    if(horizontal_0)
    {

#line 237
        luma_1_0 = _S4;

#line 237
    }
    else
    {

#line 237
        luma_1_0 = _S6;

#line 237
    }

#line 237
    float luma_2_0;
    if(horizontal_0)
    {

#line 238
        luma_2_0 = _S5;

#line 238
    }
    else
    {

#line 238
        luma_2_0 = _S7;

#line 238
    }


    float _S15 = abs(luma_1_0 - luma_c_0);

#line 241
    float _S16 = abs(luma_2_0 - luma_c_0);

#line 241
    bool steeper_1_0 = _S15 >= _S16;
    float gradient_scaled_0 = 0.25f * max(_S15, _S16);

#line 242
    float texel_0;

#line 247
    if(horizontal_0)
    {

#line 247
        texel_0 = (&kernelContext_3)->params_0->inv_source_0.y;

#line 247
    }
    else
    {

#line 247
        texel_0 = (&kernelContext_3)->params_0->inv_source_0.x;

#line 247
    }

#line 247
    float luma_local_0;
    if(steeper_1_0)
    {

#line 248
        luma_local_0 = 0.5f * (luma_1_0 + luma_c_0);

#line 248
    }
    else
    {

#line 248
        luma_local_0 = 0.5f * (luma_2_0 + luma_c_0);

#line 248
    }

#line 248
    float step_length_0;
    if(steeper_1_0)
    {

#line 249
        step_length_0 = - texel_0;

#line 249
    }
    else
    {

#line 249
        step_length_0 = texel_0;

#line 249
    }

    thread float2 edge_uv_0 = _S2.uv_3;
    if(horizontal_0)
    {
        edge_uv_0.y = edge_uv_0.y + step_length_0 * 0.5f;

#line 252
    }
    else
    {



        edge_uv_0.x = edge_uv_0.x + step_length_0 * 0.5f;

#line 252
    }

#line 252
    float2 along_0;

#line 263
    if(horizontal_0)
    {

#line 263
        along_0 = float2((&kernelContext_3)->params_0->inv_source_0.x, 0.0f);

#line 263
    }
    else
    {

#line 263
        along_0 = float2(0.0f, (&kernelContext_3)->params_0->inv_source_0.y);

#line 263
    }


    float2 uv_neg_0 = edge_uv_0 - along_0;
    float2 uv_pos_0 = edge_uv_0 + along_0;
    float delta_neg_0 = luma_of_0((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (uv_neg_0), level((0.0f)))).xyz) - luma_local_0;
    float delta_pos_0 = luma_of_0((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (uv_pos_0), level((0.0f)))).xyz) - luma_local_0;
    bool _S17 = (abs(delta_neg_0)) >= gradient_scaled_0;
    bool _S18 = (abs(delta_pos_0)) >= gradient_scaled_0;
    if(horizontal_0)
    {

#line 272
        luma_1_0 = _S2.uv_3.x - uv_neg_0.x;

#line 272
    }
    else
    {

#line 272
        luma_1_0 = _S2.uv_3.y - uv_neg_0.y;

#line 272
    }
    if(horizontal_0)
    {

#line 273
        luma_2_0 = uv_pos_0.x - _S2.uv_3.x;

#line 273
    }
    else
    {

#line 273
        luma_2_0 = uv_pos_0.y - _S2.uv_3.y;

#line 273
    }

#line 273
    bool done_neg_0 = _S17;

#line 273
    bool done_pos_0 = _S18;

#line 273
    float distance_neg_0 = luma_1_0;

#line 273
    float distance_pos_0 = luma_2_0;

#line 273
    float delta_neg_1 = delta_neg_0;

#line 273
    float delta_pos_1 = delta_pos_0;

#line 273
    int i_0 = int(0);

#line 273
    float2 uv_neg_1 = uv_neg_0;

#line 273
    float2 uv_pos_1 = uv_pos_0;



    for(;;)
    {

#line 277
        if(i_0 < int(12))
        {
        }
        else
        {

#line 277
            break;
        }

#line 277
        int _S19 = i_0;


        if(!done_neg_0)
        {
            float2 uv_neg_2 = uv_neg_1 - along_0 * float2(SEARCH_STEP_0[_S19]) ;
            float delta_neg_2 = luma_of_0((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (uv_neg_2), level((0.0f)))).xyz) - luma_local_0;
            bool _S20 = (abs(delta_neg_2)) >= gradient_scaled_0;
            if(horizontal_0)
            {

#line 285
                luma_1_0 = _S2.uv_3.x - uv_neg_2.x;

#line 285
            }
            else
            {

#line 285
                luma_1_0 = _S2.uv_3.y - uv_neg_2.y;

#line 285
            }

#line 285
            done_neg_0 = _S20;

#line 285
            distance_neg_0 = luma_1_0;

#line 285
            delta_neg_1 = delta_neg_2;

#line 285
            uv_neg_1 = uv_neg_2;

#line 280
        }

#line 287
        if(!done_pos_0)
        {
            float2 uv_pos_2 = uv_pos_1 + along_0 * float2(SEARCH_STEP_0[_S19]) ;
            float delta_pos_2 = luma_of_0((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (uv_pos_2), level((0.0f)))).xyz) - luma_local_0;
            bool _S21 = (abs(delta_pos_2)) >= gradient_scaled_0;
            if(horizontal_0)
            {

#line 292
                luma_1_0 = uv_pos_2.x - _S2.uv_3.x;

#line 292
            }
            else
            {

#line 292
                luma_1_0 = uv_pos_2.y - _S2.uv_3.y;

#line 292
            }

#line 292
            done_pos_0 = _S21;

#line 292
            distance_pos_0 = luma_1_0;

#line 292
            delta_pos_1 = delta_pos_2;

#line 292
            uv_pos_1 = uv_pos_2;

#line 287
        }

#line 277
        i_0 = i_0 + int(1);

#line 277
    }

#line 300
    float _S22 = max(0.0f, 0.5f - min(distance_neg_0, distance_pos_0) / max(distance_neg_0 + distance_pos_0, 9.99999997475242708e-07f)) * step_length_0;

#line 300
    float delta_nearer_0;

#line 306
    if(distance_neg_0 < distance_pos_0)
    {

#line 306
        delta_nearer_0 = delta_neg_1;

#line 306
    }
    else
    {

#line 306
        delta_nearer_0 = delta_pos_1;

#line 306
    }

#line 306
    float offset_3;
    if(((luma_c_0 - luma_local_0) < 0.0f) == (delta_nearer_0 < 0.0f))
    {

#line 307
        offset_3 = 0.0f;

#line 307
    }
    else
    {

#line 307
        offset_3 = _S22;

#line 307
    }

#line 315
    float subpixel_ratio_0 = saturate(abs((2.0f * (luma_ns_0 + luma_we_0) + luma_wcorners_0 + luma_ecorners_0) * 0.0833333358168602f - luma_c_0) / max(range_0, 9.99999997475242708e-07f));
    float subpixel_weight_0 = (-2.0f * subpixel_ratio_0 + 3.0f) * subpixel_ratio_0 * subpixel_ratio_0;
    float subpixel_offset_0 = subpixel_weight_0 * subpixel_weight_0 * (&kernelContext_3)->params_0->subpixel_0 * step_length_0;

#line 317
    float final_offset_0;



    if((abs(subpixel_offset_0)) > (abs(offset_3)))
    {

#line 321
        final_offset_0 = subpixel_offset_0;

#line 321
    }
    else
    {

#line 321
        final_offset_0 = offset_3;

#line 321
    }

    thread float2 result_uv_0 = _S2.uv_3;
    if(horizontal_0)
    {
        result_uv_0.y = result_uv_0.y + final_offset_0;

#line 324
    }
    else
    {



        result_uv_0.x = result_uv_0.x + final_offset_0;

#line 324
    }

#line 324
    pixelOutput_0 _S23 = { float4((((&kernelContext_3)->source_0).sample(((&kernelContext_3)->sourceSampler_0), (result_uv_0), level((0.0f)))).xyz, 1.0f) };

#line 333
    return _S23;
}


#line 333
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_4 [[user(TEXCOORD)]];
};


#line 122
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_5;
};


#line 122
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], FxaaParams_0 constant* params_2 [[buffer(0)]], texture2d<float, access::sample> source_2 [[texture(0)]], sampler sourceSampler_2 [[sampler(0)]])
{

#line 122
    thread KernelContext_0 kernelContext_4;

#line 122
    (&kernelContext_4)->params_0 = params_2;

#line 122
    (&kernelContext_4)->source_0 = source_2;

#line 122
    (&kernelContext_4)->sourceSampler_0 = sourceSampler_2;

#line 179
    thread FullscreenOutput_0 output_1;


    float2 _S24 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 182
    (&output_1)->uv_5 = _S24;
    (&output_1)->position_2 = float4(_S24 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 183
    thread vertexMain_Result_0 _S25;

#line 183
    (&_S25)->position_1 = output_1.position_2;

#line 183
    (&_S25)->uv_4 = output_1.uv_5;

#line 183
    return _S25;
}

