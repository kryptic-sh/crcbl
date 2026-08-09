#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 43 "shaders/triangle.slang"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 43
struct pixelInput_0
{
    float4 color_0 [[user(COLOR)]];
};


#line 43
struct Vertex_natural_0
{
    packed_float4 position_0;
    packed_float4 color_1;
};


#line 70
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_1 [[position]], Vertex_natural_0 device* vertices_0 [[buffer(0)]])
{

#line 70
    pixelOutput_0 _S2 = { _S1.color_0 };

    return _S2;
}


#line 72
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float4 color_2 [[user(COLOR)]];
};


#line 43
struct VertexOutput_0
{
    float4 position_3;
    float4 color_3;
};


#line 43
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], Vertex_natural_0 device* vertices_1 [[buffer(0)]])
{

#line 61
    Vertex_natural_0 vertex_0 = vertices_1[index_0];

    thread VertexOutput_0 output_1;
    (&output_1)->position_3 = float4(vertex_0.position_0) ;
    (&output_1)->color_3 = float4(vertex_0.color_1) ;

#line 65
    thread vertexMain_Result_0 _S3;

#line 65
    (&_S3)->position_2 = output_1.position_3;

#line 65
    (&_S3)->color_2 = output_1.color_3;

#line 65
    return _S3;
}

