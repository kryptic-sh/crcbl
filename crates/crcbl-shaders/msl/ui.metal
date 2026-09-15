#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 161 "shaders/ui.slang"
float2 sharpen_0(float2 uv_0, float2 size_0)
{

#line 161
    float2 _S1 = float2(0.5f) ;

    float2 s_0 = uv_0 * size_0 - _S1;
    float2 whole_0 = floor(s_0);



    return (whole_0 + saturate((s_0 - whole_0 - _S1) / max((fwidth((s_0))), float2(9.99999997475242708e-07f, 9.99999997475242708e-07f)) + _S1) + _S1) / size_0;
}


#line 178
float roundedBoxDistance_0(float2 p_0, float2 halfExtent_0, float4 radii_0)
{
    bool _S2 = (p_0.x) > 0.0f;

#line 180
    float top_0;

#line 180
    if(_S2)
    {

#line 180
        top_0 = radii_0.y;

#line 180
    }
    else
    {

#line 180
        top_0 = radii_0.x;

#line 180
    }

#line 180
    float bottom_0;
    if(_S2)
    {

#line 181
        bottom_0 = radii_0.z;

#line 181
    }
    else
    {

#line 181
        bottom_0 = radii_0.w;

#line 181
    }

#line 181
    float radius_0;
    if((p_0.y) > 0.0f)
    {

#line 182
        radius_0 = bottom_0;

#line 182
    }
    else
    {

#line 182
        radius_0 = top_0;

#line 182
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


#line 56 "shaders/ui.slang"
struct UiConstants_0
{
    float2 viewport_0;
};


#line 56
struct KernelContext_0
{
    Vertex_natural_0 device* vertices_0;
    UiConstants_0 constant* constants_0;
    texture2d<float, access::sample> glyphAtlas_0;
    sampler glyphSampler_0;
    texture2d<float, access::sample> imageAtlas_0;
    sampler imageSampler_0;
};


#line 188
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S3 [[stage_in]], float4 position_1 [[position]], Vertex_natural_0 device* vertices_1 [[buffer(0)]], UiConstants_0 constant* constants_1 [[buffer(1)]], texture2d<float, access::sample> glyphAtlas_1 [[texture(0)]], sampler glyphSampler_1 [[sampler(0)]], texture2d<float, access::sample> imageAtlas_1 [[texture(1)]], sampler imageSampler_1 [[sampler(1)]])
{

#line 188
    thread KernelContext_0 kernelContext_0;

#line 188
    (&kernelContext_0)->vertices_0 = vertices_1;

#line 188
    (&kernelContext_0)->constants_0 = constants_1;

#line 188
    (&kernelContext_0)->glyphAtlas_0 = glyphAtlas_1;

#line 188
    (&kernelContext_0)->glyphSampler_0 = glyphSampler_1;

#line 188
    (&kernelContext_0)->imageAtlas_0 = imageAtlas_1;

#line 188
    (&kernelContext_0)->imageSampler_0 = imageSampler_1;

#line 193
    float glyph_0 = ((glyphAtlas_1).sample((glyphSampler_1), (_S3.uv_1)).x);
    thread uint imageWidth_0;
    thread uint imageHeight_0;
    (*((&imageWidth_0)) = (imageAtlas_1).get_width(0)),(*((&imageHeight_0)) = (imageAtlas_1).get_height(0));

    float4 texel_0 = ((imageAtlas_1).sample((imageSampler_1), (sharpen_0(_S3.uv_1, float2(float(imageWidth_0), float(imageHeight_0))))));

    float primitive_0 = _S3.shape_0.w;
    thread float4 color_2 = _S3.color_0;

#line 201
    float _S4;


    if(primitive_0 == 1.0f)
    {

#line 204
        _S4 = glyph_0;

#line 204
    }
    else
    {

#line 204
        _S4 = 1.0f;

#line 204
    }

#line 204
    color_2.w = color_2.w * _S4;

#line 204
    float4 _S5;


    if(primitive_0 == 2.0f)
    {

#line 207
        _S5 = texel_0 * _S3.color_0;

#line 207
    }
    else
    {

#line 207
        _S5 = color_2;

#line 207
    }

#line 207
    color_2 = _S5;

#line 214
    float distance_0 = roundedBoxDistance_0(_S3.uv_1, _S3.shape_0.xy, _S3.radii_1);
    float coverage_0 = saturate(0.5f - distance_0);
    float _S6 = _S3.shape_0.z;

#line 216
    float inner_0 = saturate(0.5f - (distance_0 + _S6));
    thread float4 rounded_0;

#line 217
    if(_S6 > 0.0f)
    {

#line 217
        _S4 = inner_0;

#line 217
    }
    else
    {

#line 217
        _S4 = 1.0f;

#line 217
    }

#line 217
    rounded_0 = mix(_S3.border_0, _S3.color_0, float4(_S4) );
    rounded_0.w = rounded_0.w * coverage_0;
    if(primitive_0 == 3.0f)
    {

#line 219
        _S5 = rounded_0;

#line 219
    }
    else
    {

#line 219
        _S5 = color_2;

#line 219
    }

#line 219
    color_2 = _S5;



    float _S7 = _S3.screen_0.x;

#line 223
    bool _S8;

#line 223
    if(_S7 < (_S3.clip_0.x))
    {

#line 223
        _S8 = true;

#line 223
    }
    else
    {

#line 223
        _S8 = (_S3.screen_0.y) < (_S3.clip_0.y);

#line 223
    }
    if(_S8)
    {

#line 224
        _S8 = true;

#line 224
    }
    else
    {

#line 224
        _S8 = _S7 >= (_S3.clip_0.z);

#line 224
    }

#line 224
    if(_S8)
    {

#line 224
        _S8 = true;

#line 224
    }
    else
    {

#line 224
        _S8 = (_S3.screen_0.y) >= (_S3.clip_0.w);

#line 224
    }

#line 223
    if(_S8)
    {

        discard_fragment();

#line 223
    }

#line 223
    pixelOutput_0 _S9 = { color_2 };

#line 229
    return _S9;
}


#line 229
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


#line 114
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


#line 114
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], Vertex_natural_0 device* vertices_2 [[buffer(0)]], UiConstants_0 constant* constants_2 [[buffer(1)]], texture2d<float, access::sample> glyphAtlas_2 [[texture(0)]], sampler glyphSampler_2 [[sampler(0)]], texture2d<float, access::sample> imageAtlas_2 [[texture(1)]], sampler imageSampler_2 [[sampler(1)]])
{

#line 114
    thread KernelContext_0 kernelContext_1;

#line 114
    (&kernelContext_1)->vertices_0 = vertices_2;

#line 114
    (&kernelContext_1)->constants_0 = constants_2;

#line 114
    (&kernelContext_1)->glyphAtlas_0 = glyphAtlas_2;

#line 114
    (&kernelContext_1)->glyphSampler_0 = glyphSampler_2;

#line 114
    (&kernelContext_1)->imageAtlas_0 = imageAtlas_2;

#line 114
    (&kernelContext_1)->imageSampler_0 = imageSampler_2;

#line 135
    Vertex_natural_0 v_0 = vertices_2[index_0];



    thread float2 ndc_0;

#line 139
    float2 _S10 = float2(v_0.position_0) ;
    ndc_0.x = _S10.x / constants_2->viewport_0.x * 2.0f - 1.0f;
    ndc_0.y = 1.0f - _S10.y / constants_2->viewport_0.y * 2.0f;

    thread UiOutput_0 output_1;
    (&output_1)->position_3 = float4(ndc_0, 0.0f, 1.0f);
    (&output_1)->uv_4 = float2(v_0.uv_2) ;
    (&output_1)->color_4 = float4(v_0.color_1) ;
    (&output_1)->screen_2 = _S10;
    (&output_1)->clip_3 = float4(v_0.clip_1) ;
    (&output_1)->shape_3 = float4(v_0.shape_1) ;
    (&output_1)->radii_4 = float4(v_0.radii_2) ;
    (&output_1)->border_3 = float4(v_0.border_1) ;

#line 151
    thread vertexMain_Result_0 _S11;

#line 151
    (&_S11)->position_2 = output_1.position_3;

#line 151
    (&_S11)->uv_3 = output_1.uv_4;

#line 151
    (&_S11)->color_3 = output_1.color_4;

#line 151
    (&_S11)->screen_1 = output_1.screen_2;

#line 151
    (&_S11)->clip_2 = output_1.clip_3;

#line 151
    (&_S11)->shape_2 = output_1.shape_3;

#line 151
    (&_S11)->radii_3 = output_1.radii_4;

#line 151
    (&_S11)->border_2 = output_1.border_3;

#line 151
    return _S11;
}

