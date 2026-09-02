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
    float3 world_0 [[user(WORLD)]];
};


#line 75 "shaders/probe_capture.slang"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 75
struct CaptureFace_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 view_proj_0;
    float4 origin_0;
};


#line 75
struct KernelContext_0
{
    float device* positions_0;
    CaptureFace_natural_0 constant* capture_0;
};




[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], float device* positions_1 [[buffer(1)]], CaptureFace_natural_0 constant* capture_1 [[buffer(0)]])
{

#line 84
    thread KernelContext_0 kernelContext_0;

#line 84
    (&kernelContext_0)->positions_0 = positions_1;

#line 84
    (&kernelContext_0)->capture_0 = capture_1;

#line 84
    pixelOutput_0 _S2 = { float4(length(_S1.world_0 - capture_1->origin_0.xyz), 0.0f, 0.0f, 0.0f) };

#line 90
    return _S2;
}


#line 90
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float3 world_1 [[user(WORLD)]];
};


#line 65
struct CaptureVertex_0
{
    float4 position_2;
    float3 world_2;
};


#line 65
[[vertex]] vertexMain_Result_0 vertexMain(uint vertex_0 [[vertex_id]], float device* positions_2 [[buffer(1)]], CaptureFace_natural_0 constant* capture_2 [[buffer(0)]])
{

#line 65
    thread KernelContext_0 kernelContext_1;

#line 65
    (&kernelContext_1)->positions_0 = positions_2;

#line 65
    (&kernelContext_1)->capture_0 = capture_2;

#line 74
    uint at_0 = vertex_0 * 3U;
    float3 world_3 = float3(positions_2[at_0], positions_2[at_0 + 1U], positions_2[at_0 + 2U]);

    thread CaptureVertex_0 output_1;
    (&output_1)->world_2 = world_3;
    (&output_1)->position_2 = (((float4(world_3, 1.0f)) * (matrix<float,int(4),int(4)> (capture_2->view_proj_0.data_0[int(0)][int(0)], capture_2->view_proj_0.data_0[int(1)][int(0)], capture_2->view_proj_0.data_0[int(2)][int(0)], capture_2->view_proj_0.data_0[int(3)][int(0)], capture_2->view_proj_0.data_0[int(0)][int(1)], capture_2->view_proj_0.data_0[int(1)][int(1)], capture_2->view_proj_0.data_0[int(2)][int(1)], capture_2->view_proj_0.data_0[int(3)][int(1)], capture_2->view_proj_0.data_0[int(0)][int(2)], capture_2->view_proj_0.data_0[int(1)][int(2)], capture_2->view_proj_0.data_0[int(2)][int(2)], capture_2->view_proj_0.data_0[int(3)][int(2)], capture_2->view_proj_0.data_0[int(0)][int(3)], capture_2->view_proj_0.data_0[int(1)][int(3)], capture_2->view_proj_0.data_0[int(2)][int(3)], capture_2->view_proj_0.data_0[int(3)][int(3)]))));

#line 79
    thread vertexMain_Result_0 _S3;

#line 79
    (&_S3)->position_1 = output_1.position_2;

#line 79
    (&_S3)->world_1 = output_1.world_2;

#line 79
    return _S3;
}

