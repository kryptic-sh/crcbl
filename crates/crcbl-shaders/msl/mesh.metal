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
    float3 world_position_0 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_0 [[user(COLOR)]];
};


#line 90
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_1;
};


#line 90
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 90
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 view_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 model_0;
    float4 camera_position_0;
    float4 light_direction_0;
    float4 light_color_0;
    float4 ambient_0;
};


#line 90
struct KernelContext_0
{
    MeshVertex_natural_0 device* vertices_0;
    FrameUniforms_natural_0 constant* frame_0;
};


#line 126 "shaders/mesh.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_1 [[position]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]])
{

#line 126
    thread KernelContext_0 kernelContext_0;

#line 126
    (&kernelContext_0)->vertices_0 = vertices_1;

#line 126
    (&kernelContext_0)->frame_0 = frame_1;



    float3 normal_1 = normalize(_S1.world_normal_0);
    float3 to_light_0 = normalize(frame_1->light_direction_0.xyz);



    float _S2 = max(dot(normal_1, to_light_0), 0.0f);

#line 135
    pixelOutput_0 _S3 = { float4(_S1.color_0.xyz * (frame_1->ambient_0.xyz + frame_1->light_color_0.xyz * float3(_S2) ) + frame_1->light_color_0.xyz * float3((pow(max(dot(normal_1, normalize(to_light_0 + normalize(frame_1->camera_position_0.xyz - _S1.world_position_0))), 0.0f), 32.0f) * (step(0.0f, _S2) * _S2) * 0.34999999403953552f)) , _S1.color_0.w) };

#line 150
    return _S3;
}


#line 150
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float3 world_position_1 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
};


#line 87
struct VertexOutput_0
{
    float4 position_3;
    float3 world_position_2;
    float3 world_normal_2;
    float4 color_3;
};


#line 87
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]])
{

#line 87
    thread KernelContext_0 kernelContext_1;

#line 87
    (&kernelContext_1)->vertices_0 = vertices_2;

#line 87
    (&kernelContext_1)->frame_0 = frame_2;

#line 98
    MeshVertex_natural_0 vertex_0 = vertices_2[index_0];

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (matrix<float,int(4),int(4)> (frame_2->model_0.data_0[int(0)][int(0)], frame_2->model_0.data_0[int(1)][int(0)], frame_2->model_0.data_0[int(2)][int(0)], frame_2->model_0.data_0[int(3)][int(0)], frame_2->model_0.data_0[int(0)][int(1)], frame_2->model_0.data_0[int(1)][int(1)], frame_2->model_0.data_0[int(2)][int(1)], frame_2->model_0.data_0[int(3)][int(1)], frame_2->model_0.data_0[int(0)][int(2)], frame_2->model_0.data_0[int(1)][int(2)], frame_2->model_0.data_0[int(2)][int(2)], frame_2->model_0.data_0[int(3)][int(2)], frame_2->model_0.data_0[int(0)][int(3)], frame_2->model_0.data_0[int(1)][int(3)], frame_2->model_0.data_0[int(2)][int(3)], frame_2->model_0.data_0[int(3)][int(3)]))));

    thread VertexOutput_0 output_1;
    (&output_1)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_0[int(0)][int(0)], frame_2->view_proj_0.data_0[int(1)][int(0)], frame_2->view_proj_0.data_0[int(2)][int(0)], frame_2->view_proj_0.data_0[int(3)][int(0)], frame_2->view_proj_0.data_0[int(0)][int(1)], frame_2->view_proj_0.data_0[int(1)][int(1)], frame_2->view_proj_0.data_0[int(2)][int(1)], frame_2->view_proj_0.data_0[int(3)][int(1)], frame_2->view_proj_0.data_0[int(0)][int(2)], frame_2->view_proj_0.data_0[int(1)][int(2)], frame_2->view_proj_0.data_0[int(2)][int(2)], frame_2->view_proj_0.data_0[int(3)][int(2)], frame_2->view_proj_0.data_0[int(0)][int(3)], frame_2->view_proj_0.data_0[int(1)][int(3)], frame_2->view_proj_0.data_0[int(2)][int(3)], frame_2->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_1)->world_position_2 = world_0.xyz;

#line 104
    matrix<float,int(4),int(4)>  _S4 = matrix<float,int(4),int(4)> (frame_2->model_0.data_0[int(0)][int(0)], frame_2->model_0.data_0[int(1)][int(0)], frame_2->model_0.data_0[int(2)][int(0)], frame_2->model_0.data_0[int(3)][int(0)], frame_2->model_0.data_0[int(0)][int(1)], frame_2->model_0.data_0[int(1)][int(1)], frame_2->model_0.data_0[int(2)][int(1)], frame_2->model_0.data_0[int(3)][int(1)], frame_2->model_0.data_0[int(0)][int(2)], frame_2->model_0.data_0[int(1)][int(2)], frame_2->model_0.data_0[int(2)][int(2)], frame_2->model_0.data_0[int(3)][int(2)], frame_2->model_0.data_0[int(0)][int(3)], frame_2->model_0.data_0[int(1)][int(3)], frame_2->model_0.data_0[int(2)][int(3)], frame_2->model_0.data_0[int(3)][int(3)]);

#line 109
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S4[int(0)].xyz, _S4[int(1)].xyz, _S4[int(2)].xyz))));
    (&output_1)->color_3 = float4(vertex_0.color_1) ;

#line 110
    thread vertexMain_Result_0 _S5;

#line 110
    (&_S5)->position_2 = output_1.position_3;

#line 110
    (&_S5)->world_position_1 = output_1.world_position_2;

#line 110
    (&_S5)->world_normal_1 = output_1.world_normal_2;

#line 110
    (&_S5)->color_2 = output_1.color_3;

#line 110
    return _S5;
}

