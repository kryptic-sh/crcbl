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


#line 225 "shaders/mesh.slang"
struct DrawConstants_0
{
    uint base_instance_0;
    uint pad0_0;
    uint pad1_0;
    uint pad2_0;
};


#line 225
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 225
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 198
struct GpuMesh_0
{
    uint base_vertex_0;
    uint base_index_0;
    uint index_count_0;
    float min_x_0;
    float min_y_0;
    float min_z_0;
    float max_x_0;
    float max_y_0;
    float max_z_0;
};


#line 272
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_1;
};


#line 272
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 272
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 light_direction_0;
    float4 light_color_0;
    float4 ambient_0;
};


#line 272
struct KernelContext_0
{
    DrawConstants_0 constant* draw_0;
    GpuInstance_natural_0 device* instances_0;
    GpuMesh_0 device* meshes_0;
    MeshVertex_natural_0 device* vertices_0;
    FrameUniforms_natural_0 constant* frame_0;
};


#line 301
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_1 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]])
{

#line 301
    thread KernelContext_0 kernelContext_0;

#line 301
    (&kernelContext_0)->draw_0 = draw_1;

#line 301
    (&kernelContext_0)->instances_0 = instances_1;

#line 301
    (&kernelContext_0)->meshes_0 = meshes_1;

#line 301
    (&kernelContext_0)->vertices_0 = vertices_1;

#line 301
    (&kernelContext_0)->frame_0 = frame_1;



    float3 normal_1 = normalize(_S1.world_normal_0);
    float3 to_light_0 = normalize(frame_1->light_direction_0.xyz);



    float _S2 = max(dot(normal_1, to_light_0), 0.0f);

#line 310
    pixelOutput_0 _S3 = { float4(_S1.color_0.xyz * (frame_1->ambient_0.xyz + frame_1->light_color_0.xyz * float3(_S2) ) + frame_1->light_color_0.xyz * float3((pow(max(dot(normal_1, normalize(to_light_0 + normalize(frame_1->camera_position_0.xyz - _S1.world_position_0))), 0.0f), 32.0f) * (step(0.0f, _S2) * _S2) * 0.34999999403953552f)) , _S1.color_0.w) };

#line 325
    return _S3;
}


#line 325
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float3 world_position_1 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
};


#line 257
struct VertexOutput_0
{
    float4 position_3;
    float3 world_position_2;
    float3 world_normal_2;
    float4 color_3;
};


#line 257
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], uint instance_index_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]])
{

#line 257
    thread KernelContext_0 kernelContext_1;

#line 257
    (&kernelContext_1)->draw_0 = draw_2;

#line 257
    (&kernelContext_1)->instances_0 = instances_2;

#line 257
    (&kernelContext_1)->meshes_0 = meshes_2;

#line 257
    (&kernelContext_1)->vertices_0 = vertices_2;

#line 257
    (&kernelContext_1)->frame_0 = frame_2;

#line 268
    GpuInstance_natural_0 instance_0 = instances_2[instance_index_0 + draw_2->base_instance_0];

#line 273
    MeshVertex_natural_0 vertex_0 = vertices_2[index_0 + meshes_2[instance_0.mesh_0].base_vertex_0];

#line 273
    matrix<float,int(4),int(4)>  _S4 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S4)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_2 = world_0.xyz;

#line 284
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S4[int(0)].xyz, _S4[int(1)].xyz, _S4[int(2)].xyz))));
    (&output_1)->color_3 = float4(vertex_0.color_1) ;

#line 285
    thread vertexMain_Result_0 _S5;

#line 285
    (&_S5)->position_2 = output_1.position_3;

#line 285
    (&_S5)->world_position_1 = output_1.world_position_2;

#line 285
    (&_S5)->world_normal_1 = output_1.world_normal_2;

#line 285
    (&_S5)->color_2 = output_1.color_3;

#line 285
    return _S5;
}

