#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2580 "core.meta.slang"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 2580
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
    float4 color_0 [[user(COLOR)]];
};


#line 2580
struct Vertex_natural_0
{
    packed_float2 position_0;
    packed_float2 uv_1;
    packed_float4 color_1;
};


#line 19 "shaders/ui.slang"
struct UiConstants_0
{
    float2 viewport_0;
};


#line 5522 "core.meta.slang"
struct KernelContext_0
{
    Vertex_natural_0 device* vertices_0;
    UiConstants_0 constant* constants_0;
    texture2d<float, access::sample> glyphAtlas_0;
    sampler glyphSampler_0;
};


#line 73 "shaders/ui.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_1 [[position]], Vertex_natural_0 device* vertices_1 [[buffer(1)]], UiConstants_0 constant* constants_1 [[buffer(0)]], texture2d<float, access::sample> glyphAtlas_1 [[texture(0)]], sampler glyphSampler_1 [[sampler(0)]])
{

#line 73
    thread KernelContext_0 kernelContext_0;

#line 73
    (&kernelContext_0)->vertices_0 = vertices_1;

#line 73
    (&kernelContext_0)->constants_0 = constants_1;

#line 73
    (&kernelContext_0)->glyphAtlas_0 = glyphAtlas_1;

#line 73
    (&kernelContext_0)->glyphSampler_0 = glyphSampler_1;

    thread float4 color_2 = _S1.color_0;

#line 88
    float glyph_0 = ((glyphAtlas_1).sample((glyphSampler_1), (_S1.uv_0)).x);

#line 88
    bool textured_0;
    if((_S1.uv_0.x) > 0.0f)
    {

#line 89
        textured_0 = true;

#line 89
    }
    else
    {

#line 89
        textured_0 = (_S1.uv_0.y) > 0.0f;

#line 89
    }

#line 89
    float _S2;
    if(textured_0)
    {

#line 90
        _S2 = glyph_0;

#line 90
    }
    else
    {

#line 90
        _S2 = 1.0f;

#line 90
    }

#line 90
    color_2.w = color_2.w * _S2;

#line 90
    pixelOutput_0 _S3 = { color_2 };

    return _S3;
}


#line 92
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float2 uv_2 [[user(TEXCOORD)]];
    float4 color_3 [[user(COLOR)]];
};


#line 47
struct UiOutput_0
{
    float4 position_3;
    float2 uv_3;
    float4 color_4;
};


#line 47
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], Vertex_natural_0 device* vertices_2 [[buffer(1)]], UiConstants_0 constant* constants_2 [[buffer(0)]], texture2d<float, access::sample> glyphAtlas_2 [[texture(0)]], sampler glyphSampler_2 [[sampler(0)]])
{

#line 47
    thread KernelContext_0 kernelContext_1;

#line 47
    (&kernelContext_1)->vertices_0 = vertices_2;

#line 47
    (&kernelContext_1)->constants_0 = constants_2;

#line 47
    (&kernelContext_1)->glyphAtlas_0 = glyphAtlas_2;

#line 47
    (&kernelContext_1)->glyphSampler_0 = glyphSampler_2;

#line 57
    Vertex_natural_0 v_0 = vertices_2[index_0];



    thread float2 ndc_0;

#line 61
    float2 _S4 = float2(v_0.position_0) ;
    ndc_0.x = _S4.x / constants_2->viewport_0.x * 2.0f - 1.0f;
    ndc_0.y = 1.0f - _S4.y / constants_2->viewport_0.y * 2.0f;

    thread UiOutput_0 output_1;
    (&output_1)->position_3 = float4(ndc_0, 0.0f, 1.0f);
    (&output_1)->uv_3 = float2(v_0.uv_1) ;
    (&output_1)->color_4 = float4(v_0.color_1) ;

#line 68
    thread vertexMain_Result_0 _S5;

#line 68
    (&_S5)->position_2 = output_1.position_3;

#line 68
    (&_S5)->uv_2 = output_1.uv_3;

#line 68
    (&_S5)->color_3 = output_1.color_4;

#line 68
    return _S5;
}

