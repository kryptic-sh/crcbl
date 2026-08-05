#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 177 "shaders/sprite.slang"
constant array<float2, int(6)> CORNERS_0 = { float2(0.0f, 0.0f), float2(1.0f, 0.0f), float2(0.0f, 1.0f), float2(0.0f, 1.0f), float2(1.0f, 0.0f), float2(1.0f, 1.0f) };

#line 393
float2 sharpen_0(float2 uv_0, float2 size_0)
{

#line 393
    float2 _S1 = float2(0.5f) ;


    float2 s_0 = uv_0 * size_0 - _S1;
    float2 whole_0 = floor(s_0);

#line 406
    return (whole_0 + saturate((s_0 - whole_0 - _S1) / max((fwidth((s_0))), float2(9.99999997475242708e-07f, 9.99999997475242708e-07f)) + _S1) + _S1) / size_0;
}


#line 90 "core"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 2579 "core.meta.slang"
struct pixelInput_0
{
    float2 uv_1 [[user(TEXCOORD)]];
    float4 tint_0 [[user(COLOR)]];
    [[flat]] float3 sheet_0 [[user(TEXCOORD_1)]];
};


#line 2579
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 2579
struct SpriteConstants_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 view_proj_0;
    float2 viewport_0;
    uint base_0;
    uint pad_0;
};


#line 2579
struct SpriteInstance_natural_0
{
    packed_float4 rect_0;
    packed_float4 uv_2;
    packed_float4 tint_1;
    packed_float4 sheet_1;
};


#line 2579
struct KernelContext_0
{
    SpriteConstants_natural_0 constant* constants_0;
    SpriteInstance_natural_0 device* sprites_0;
    texture2d<float, access::sample> sheet_2;
    sampler sheetSampler_0;
};


#line 410 "shaders/sprite.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], SpriteConstants_natural_0 constant* constants_1 [[buffer(0)]], SpriteInstance_natural_0 device* sprites_1 [[buffer(1)]], texture2d<float, access::sample> sheet_3 [[texture(0)]], sampler sheetSampler_1 [[sampler(0)]])
{

#line 410
    thread KernelContext_0 kernelContext_0;

#line 410
    (&kernelContext_0)->constants_0 = constants_1;

#line 410
    (&kernelContext_0)->sprites_0 = sprites_1;

#line 410
    (&kernelContext_0)->sheet_2 = sheet_3;

#line 410
    (&kernelContext_0)->sheetSampler_0 = sheetSampler_1;

#line 410
    pixelOutput_0 _S3 = { ((sheet_3).sample((sheetSampler_1), (mix(_S2.uv_1, sharpen_0(_S2.uv_1, max(_S2.sheet_0.xy, float2(1.0f, 1.0f))), float2(_S2.sheet_0.z) )))) * _S2.tint_0 };

#line 438
    return _S3;
}


#line 438
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_3 [[user(TEXCOORD)]];
    float4 tint_2 [[user(COLOR)]];
    float3 sheet_4 [[user(TEXCOORD_1)]];
};


#line 186
struct SpriteVarying_0
{
    float4 position_2;
    float2 uv_4;
    float4 tint_3;
    [[flat]] float3 sheet_5;
};


#line 186
[[vertex]] vertexMain_Result_0 vertexMain(uint vertex_0 [[vertex_id]], uint instance_0 [[instance_id]], SpriteConstants_natural_0 constant* constants_2 [[buffer(0)]], SpriteInstance_natural_0 device* sprites_2 [[buffer(1)]], texture2d<float, access::sample> sheet_6 [[texture(0)]], sampler sheetSampler_2 [[sampler(0)]])
{

#line 186
    thread KernelContext_0 kernelContext_1;

#line 186
    (&kernelContext_1)->constants_0 = constants_2;

#line 186
    (&kernelContext_1)->sprites_0 = sprites_2;

#line 186
    (&kernelContext_1)->sheet_2 = sheet_6;

#line 186
    (&kernelContext_1)->sheetSampler_0 = sheetSampler_2;

#line 208
    SpriteInstance_natural_0 s_1 = sprites_2[instance_0 + constants_2->base_0];

#line 208
    float4 _S4 = float4(s_1.rect_0) ;

#line 214
    float2 _S5 = _S4.xy;

#line 214
    float2 _S6 = _S4.zw;

#line 214
    float2 _S7 = CORNERS_0[vertex_0] * _S6;

#line 214
    float2 _S8 = _S5 + _S7;

#line 214
    float4 _S9 = float4(s_1.sheet_1) ;

#line 234
    float angle_0 = _S9.w;
    bool _S10 = angle_0 != 0.0f;

#line 235
    float2 world_0;

#line 235
    if(_S10)
    {
        float2 pivot_0 = _S6 * float2(0.5f) ;
        float2 offset_0 = _S7 - pivot_0;
        float sinA_0 = sin(angle_0);
        float cosA_0 = cos(angle_0);



        float _S11 = offset_0.x;

#line 244
        float _S12 = offset_0.y;

#line 244
        world_0 = _S5 + pivot_0 + float2(_S11 * cosA_0 - _S12 * sinA_0, _S11 * sinA_0 + _S12 * cosA_0);

#line 235
    }
    else
    {

#line 235
        world_0 = _S8;

#line 235
    }

#line 235
    float4 _S13 = float4(s_1.uv_2) ;

#line 255
    float2 uv_5 = float2(mix(_S13.x, _S13.z, CORNERS_0[vertex_0].x), mix(_S13.w, _S13.y, CORNERS_0[vertex_0].y));

#line 260
    float4 _S14 = (((float4(world_0, 0.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_1)->constants_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(3)][int(3)]))));

#line 260
    thread float4 clip_0 = _S14;

#line 298
    float pixelMode_0 = _S9.z;


    float2 viewport_1 = max((&kernelContext_1)->constants_0->viewport_0, float2(1.0f, 1.0f));
    if((_S14.w) > 0.0f)
    {

#line 302
        float2 _S15 = float2(0.5f) ;

#line 309
        float2 pixels_0 = (clip_0.xy / float2(clip_0.w)  * _S15 + _S15) * viewport_1;
        float2 _S16 = float2(pixelMode_0) ;

#line 310
        float2 _S17 = mix(pixels_0, round(pixels_0), _S16);

#line 310
        float2 snapped_0;
        if(_S10)
        {



            float4 centreClip_0 = (((float4(_S5 + _S6 * _S15, 0.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_1)->constants_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_1)->constants_0->view_proj_0.data_0[int(3)][int(3)]))));

            float _S18 = centreClip_0.w;

#line 318
            if(_S18 > 0.0f)
            {

                float2 centrePixels_0 = (centreClip_0.xy / float2(_S18)  * _S15 + _S15) * viewport_1;

#line 321
                snapped_0 = pixels_0 + (round(centrePixels_0) - centrePixels_0);

#line 318
            }
            else
            {

#line 318
                snapped_0 = pixels_0;

#line 318
            }

#line 318
            snapped_0 = mix(pixels_0, snapped_0, _S16);

#line 311
        }
        else
        {

#line 311
            snapped_0 = _S17;

#line 311
        }

#line 326
        clip_0.xy = (snapped_0 / viewport_1 * float2(2.0f)  - float2(1.0f) ) * float2(clip_0.w) ;

#line 302
    }

#line 329
    thread SpriteVarying_0 output_1;
    (&output_1)->position_2 = clip_0;
    (&output_1)->uv_4 = uv_5;
    (&output_1)->tint_3 = float4(s_1.tint_1) ;
    (&output_1)->sheet_5 = _S9.xyz;
    SpriteVarying_0 _S19 = output_1;

#line 334
    thread vertexMain_Result_0 _S20;

#line 334
    (&_S20)->position_1 = _S19.position_2;

#line 334
    (&_S20)->uv_3 = _S19.uv_4;

#line 334
    (&_S20)->tint_2 = _S19.tint_3;

#line 334
    (&_S20)->sheet_4 = _S19.sheet_5;

#line 334
    return _S20;
}

