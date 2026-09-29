#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 194 "shaders/ui.slang"
float2 sharpen_0(float2 uv_0, float2 size_0)
{

#line 194
    float2 _S1 = float2(0.5f) ;

    float2 s_0 = uv_0 * size_0 - _S1;
    float2 whole_0 = floor(s_0);



    return (whole_0 + saturate((s_0 - whole_0 - _S1) / max((fwidth((s_0))), float2(9.99999997475242708e-07f, 9.99999997475242708e-07f)) + _S1) + _S1) / size_0;
}


#line 211
float roundedBoxDistance_0(float2 p_0, float2 halfExtent_0, float4 radii_0)
{
    bool _S2 = (p_0.x) > 0.0f;

#line 213
    float top_0;

#line 213
    if(_S2)
    {

#line 213
        top_0 = radii_0.y;

#line 213
    }
    else
    {

#line 213
        top_0 = radii_0.x;

#line 213
    }

#line 213
    float bottom_0;
    if(_S2)
    {

#line 214
        bottom_0 = radii_0.z;

#line 214
    }
    else
    {

#line 214
        bottom_0 = radii_0.w;

#line 214
    }

#line 214
    float radius_0;
    if((p_0.y) > 0.0f)
    {

#line 215
        radius_0 = bottom_0;

#line 215
    }
    else
    {

#line 215
        radius_0 = top_0;

#line 215
    }
    float2 q_0 = abs(p_0) - halfExtent_0 + float2(radius_0) ;
    return min(max(q_0.x, q_0.y), 0.0f) + length(max(q_0, float2(0.0f, 0.0f))) - radius_0;
}


#line 90 "core"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 90
struct pixelInput_0
{
    float2 uv_1 [[user(TEXCOORD)]];
    float4 color_0 [[user(COLOR)]];
    float2 screen_0 [[user(TEXCOORD_1)]];
    [[flat]] float4 clip_0 [[user(TEXCOORD_2)]];
    [[flat]] float4 shape_0 [[user(TEXCOORD_3)]];
    [[flat]] float4 radii_1 [[user(TEXCOORD_4)]];
    [[flat]] float4 border_0 [[user(TEXCOORD_5)]];
};


#line 90
struct Vertex_natural_0
{
    packed_float2 position_0;
    packed_float2 uv_2;
    packed_float4 color_1;
    packed_float4 clip_1;
    packed_float4 shape_1;
    packed_float4 radii_2;
    packed_float4 border_1;
};


#line 70 "shaders/ui.slang"
struct UiConstants_0
{
    float2 viewport_0;
};


#line 136
struct KernelContext_0
{
    Vertex_natural_0 device* vertices_0;
    UiConstants_0 constant* constants_0;
    texture2d<float, access::sample> glyphAtlas_0;
    sampler glyphSampler_0;
    texture2d<float, access::sample> imageAtlas_0;
    sampler imageSampler_0;
    texture2d<float, access::sample> boundTexture_0;
    texture2d_array<float, access::sample> glyphPages_0;
};


#line 221
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S3 [[stage_in]], float4 position_1 [[position]], Vertex_natural_0 device* vertices_1 [[buffer(0)]], UiConstants_0 constant* constants_1 [[buffer(1)]], texture2d<float, access::sample> glyphAtlas_1 [[texture(0)]], sampler glyphSampler_1 [[sampler(0)]], texture2d<float, access::sample> imageAtlas_1 [[texture(1)]], sampler imageSampler_1 [[sampler(1)]], texture2d<float, access::sample> boundTexture_1 [[texture(3)]], texture2d_array<float, access::sample> glyphPages_1 [[texture(2)]])
{

#line 221
    thread KernelContext_0 kernelContext_0;

#line 221
    (&kernelContext_0)->vertices_0 = vertices_1;

#line 221
    (&kernelContext_0)->constants_0 = constants_1;

#line 221
    (&kernelContext_0)->glyphAtlas_0 = glyphAtlas_1;

#line 221
    (&kernelContext_0)->glyphSampler_0 = glyphSampler_1;

#line 221
    (&kernelContext_0)->imageAtlas_0 = imageAtlas_1;

#line 221
    (&kernelContext_0)->imageSampler_0 = imageSampler_1;

#line 221
    (&kernelContext_0)->boundTexture_0 = boundTexture_1;

#line 221
    (&kernelContext_0)->glyphPages_0 = glyphPages_1;

#line 226
    float glyph_0 = ((glyphAtlas_1).sample((glyphSampler_1), (_S3.uv_1)).x);
    thread uint imageWidth_0;
    thread uint imageHeight_0;
    (*((&imageWidth_0)) = (imageAtlas_1).get_width(0)),(*((&imageHeight_0)) = (imageAtlas_1).get_height(0));

    float4 texel_0 = ((imageAtlas_1).sample((imageSampler_1), (sharpen_0(_S3.uv_1, float2(float(imageWidth_0), float(imageHeight_0))))));


    float4 bound_0 = ((boundTexture_1).sample((imageSampler_1), (_S3.uv_1)));

    float primitive_0 = _S3.shape_0.w;


    bool _S4 = primitive_0 == 4.0f;

#line 239
    float page_0;

#line 239
    if(_S4)
    {

#line 239
        page_0 = _S3.shape_0.x;

#line 239
    }
    else
    {

#line 239
        page_0 = 0.0f;

#line 239
    }
    float3 _S5 = float3(_S3.uv_1, page_0);

#line 240
    float pageCoverage_0 = (((&kernelContext_0)->glyphPages_0).sample(((&kernelContext_0)->glyphSampler_0), ((_S5)).xy, uint(((_S5)).z)).x);

    thread float4 color_2 = _S3.color_0;



    if(primitive_0 == 1.0f)
    {

#line 246
        page_0 = glyph_0;

#line 246
    }
    else
    {

#line 246
        page_0 = 1.0f;

#line 246
    }

#line 246
    color_2.w = color_2.w * page_0;
    if(_S4)
    {

#line 247
        page_0 = pageCoverage_0;

#line 247
    }
    else
    {

#line 247
        page_0 = 1.0f;

#line 247
    }

#line 247
    color_2.w = color_2.w * page_0;

#line 247
    float4 _S6;


    if(primitive_0 == 2.0f)
    {

#line 250
        _S6 = texel_0 * _S3.color_0;

#line 250
    }
    else
    {

#line 250
        _S6 = color_2;

#line 250
    }

#line 250
    color_2 = _S6;

    if(primitive_0 == 5.0f)
    {

#line 252
        _S6 = bound_0 * _S3.color_0;

#line 252
    }
    else
    {

#line 252
        _S6 = color_2;

#line 252
    }

#line 252
    color_2 = _S6;

#line 259
    float distance_0 = roundedBoxDistance_0(_S3.uv_1, _S3.shape_0.xy, _S3.radii_1);
    float coverage_0 = saturate(0.5f - distance_0);
    float _S7 = _S3.shape_0.z;

#line 261
    float inner_0 = saturate(0.5f - (distance_0 + _S7));
    thread float4 rounded_0;

#line 262
    if(_S7 > 0.0f)
    {

#line 262
        page_0 = inner_0;

#line 262
    }
    else
    {

#line 262
        page_0 = 1.0f;

#line 262
    }

#line 262
    rounded_0 = mix(_S3.border_0, _S3.color_0, float4(page_0) );
    rounded_0.w = rounded_0.w * coverage_0;
    if(primitive_0 == 3.0f)
    {

#line 264
        _S6 = rounded_0;

#line 264
    }
    else
    {

#line 264
        _S6 = color_2;

#line 264
    }

#line 264
    color_2 = _S6;



    float _S8 = _S3.screen_0.x;

#line 268
    bool _S9;

#line 268
    if(_S8 < (_S3.clip_0.x))
    {

#line 268
        _S9 = true;

#line 268
    }
    else
    {

#line 268
        _S9 = (_S3.screen_0.y) < (_S3.clip_0.y);

#line 268
    }
    if(_S9)
    {

#line 269
        _S9 = true;

#line 269
    }
    else
    {

#line 269
        _S9 = _S8 >= (_S3.clip_0.z);

#line 269
    }

#line 269
    if(_S9)
    {

#line 269
        _S9 = true;

#line 269
    }
    else
    {

#line 269
        _S9 = (_S3.screen_0.y) >= (_S3.clip_0.w);

#line 269
    }

#line 268
    if(_S9)
    {

        discard_fragment();

#line 268
    }

#line 268
    pixelOutput_0 _S10 = { color_2 };

#line 274
    return _S10;
}


#line 274
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float2 uv_3 [[user(TEXCOORD)]];
    float4 color_3 [[user(COLOR)]];
    float2 screen_1 [[user(TEXCOORD_1)]];
    float4 clip_2 [[user(TEXCOORD_2)]];
    float4 shape_2 [[user(TEXCOORD_3)]];
    float4 radii_3 [[user(TEXCOORD_4)]];
    float4 border_2 [[user(TEXCOORD_5)]];
};


#line 147
struct UiOutput_0
{
    float4 position_3;
    float2 uv_4;
    float4 color_4;
    float2 screen_2;
    [[flat]] float4 clip_3;
    [[flat]] float4 shape_3;
    [[flat]] float4 radii_4;
    [[flat]] float4 border_3;
};


#line 147
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], Vertex_natural_0 device* vertices_2 [[buffer(0)]], UiConstants_0 constant* constants_2 [[buffer(1)]], texture2d<float, access::sample> glyphAtlas_2 [[texture(0)]], sampler glyphSampler_2 [[sampler(0)]], texture2d<float, access::sample> imageAtlas_2 [[texture(1)]], sampler imageSampler_2 [[sampler(1)]], texture2d<float, access::sample> boundTexture_2 [[texture(3)]], texture2d_array<float, access::sample> glyphPages_2 [[texture(2)]])
{

#line 147
    thread KernelContext_0 kernelContext_1;

#line 147
    (&kernelContext_1)->vertices_0 = vertices_2;

#line 147
    (&kernelContext_1)->constants_0 = constants_2;

#line 147
    (&kernelContext_1)->glyphAtlas_0 = glyphAtlas_2;

#line 147
    (&kernelContext_1)->glyphSampler_0 = glyphSampler_2;

#line 147
    (&kernelContext_1)->imageAtlas_0 = imageAtlas_2;

#line 147
    (&kernelContext_1)->imageSampler_0 = imageSampler_2;

#line 147
    (&kernelContext_1)->boundTexture_0 = boundTexture_2;

#line 147
    (&kernelContext_1)->glyphPages_0 = glyphPages_2;

#line 168
    Vertex_natural_0 v_0 = vertices_2[index_0];



    thread float2 ndc_0;

#line 172
    float2 _S11 = float2(v_0.position_0) ;
    ndc_0.x = _S11.x / constants_2->viewport_0.x * 2.0f - 1.0f;
    ndc_0.y = 1.0f - _S11.y / constants_2->viewport_0.y * 2.0f;

    thread UiOutput_0 output_1;
    (&output_1)->position_3 = float4(ndc_0, 0.0f, 1.0f);
    (&output_1)->uv_4 = float2(v_0.uv_2) ;
    (&output_1)->color_4 = float4(v_0.color_1) ;
    (&output_1)->screen_2 = _S11;
    (&output_1)->clip_3 = float4(v_0.clip_1) ;
    (&output_1)->shape_3 = float4(v_0.shape_1) ;
    (&output_1)->radii_4 = float4(v_0.radii_2) ;
    (&output_1)->border_3 = float4(v_0.border_1) ;

#line 184
    thread vertexMain_Result_0 _S12;

#line 184
    (&_S12)->position_2 = output_1.position_3;

#line 184
    (&_S12)->uv_3 = output_1.uv_4;

#line 184
    (&_S12)->color_3 = output_1.color_4;

#line 184
    (&_S12)->screen_1 = output_1.screen_2;

#line 184
    (&_S12)->clip_2 = output_1.clip_3;

#line 184
    (&_S12)->shape_2 = output_1.shape_3;

#line 184
    (&_S12)->radii_3 = output_1.radii_4;

#line 184
    (&_S12)->border_2 = output_1.border_3;

#line 184
    return _S12;
}

