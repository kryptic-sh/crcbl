#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 90 "core"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 90
struct pixelInput_0
{
    float4 color_0 [[user(COLOR)]];
};


#line 90
struct DebugVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 color_1;
};


#line 90
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 90
struct DebugConstants_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 view_proj_0;
};


#line 90
struct KernelContext_0
{
    DebugVertex_natural_0 device* vertices_0;
    DebugConstants_natural_0 constant* constants_0;
};


#line 76 "shaders/debug_draw.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_1 [[position]], DebugVertex_natural_0 device* vertices_1 [[buffer(0)]], DebugConstants_natural_0 constant* constants_1 [[buffer(1)]])
{

#line 76
    thread KernelContext_0 kernelContext_0;

#line 76
    (&kernelContext_0)->vertices_0 = vertices_1;

#line 76
    (&kernelContext_0)->constants_0 = constants_1;

#line 76
    pixelOutput_0 _S2 = { _S1.color_0 };

    return _S2;
}


#line 78
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float4 color_2 [[user(COLOR)]];
};


#line 58
struct DebugOutput_0
{
    float4 position_3;
    float4 color_3;
};


#line 70
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], DebugVertex_natural_0 device* vertices_2 [[buffer(0)]], DebugConstants_natural_0 constant* constants_2 [[buffer(1)]])
{

#line 70
    thread KernelContext_0 kernelContext_1;

#line 70
    (&kernelContext_1)->vertices_0 = vertices_2;

#line 70
    (&kernelContext_1)->constants_0 = constants_2;

#line 67
    DebugVertex_natural_0 vertex_0 = vertices_2[index_0];

    thread DebugOutput_0 output_1;
    (&output_1)->position_3 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (matrix<float,int(4),int(4)> (constants_2->view_proj_0.data_0[int(0)][int(0)], constants_2->view_proj_0.data_0[int(1)][int(0)], constants_2->view_proj_0.data_0[int(2)][int(0)], constants_2->view_proj_0.data_0[int(3)][int(0)], constants_2->view_proj_0.data_0[int(0)][int(1)], constants_2->view_proj_0.data_0[int(1)][int(1)], constants_2->view_proj_0.data_0[int(2)][int(1)], constants_2->view_proj_0.data_0[int(3)][int(1)], constants_2->view_proj_0.data_0[int(0)][int(2)], constants_2->view_proj_0.data_0[int(1)][int(2)], constants_2->view_proj_0.data_0[int(2)][int(2)], constants_2->view_proj_0.data_0[int(3)][int(2)], constants_2->view_proj_0.data_0[int(0)][int(3)], constants_2->view_proj_0.data_0[int(1)][int(3)], constants_2->view_proj_0.data_0[int(2)][int(3)], constants_2->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_1)->color_3 = float4(vertex_0.color_1) ;

#line 71
    thread vertexMain_Result_0 _S3;

#line 71
    (&_S3)->position_2 = output_1.position_3;

#line 71
    (&_S3)->color_2 = output_1.color_3;

#line 71
    return _S3;
}

