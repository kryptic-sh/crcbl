#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 90 "core"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 2578 "core.meta.slang"
struct pixelInput_0
{
    float3 world_position_0 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_0 [[user(COLOR)]];
    [[flat]] uint material_0 [[user(TEXCOORD)]];
    float2 uv_0 [[user(TEXCOORD_1)]];
};


#line 296 "shaders/mesh.slang"
struct DrawConstants_0
{
    uint base_0;
    uint pad0_0;
    uint pad1_0;
    uint pad2_0;
};


#line 422
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 422
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_1;
    uint sector_0;
    uint flags_0;
};


#line 237
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


#line 426
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_1;
    packed_float4 uv_1;
};


#line 426
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 426
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 light_direction_0;
    float4 light_color_0;
    float4 ambient_0;
};


#line 426
struct GpuMaterial_natural_0
{
    packed_float4 base_color_0;
    uint base_color_texture_0;
    uint pad0_1;
    uint pad1_1;
    uint pad2_1;
};


#line 426
struct KernelContext_0
{
    DrawConstants_0 constant* draw_0;
    uint device* visible_instances_0;
    GpuInstance_natural_0 device* instances_0;
    GpuMesh_0 device* meshes_0;
    MeshVertex_natural_0 device* vertices_0;
    FrameUniforms_natural_0 constant* frame_0;
    GpuMaterial_natural_0 device* materials_0;
    texture2d_array<float, access::sample> base_color_textures_0;
    sampler base_color_sampler_0;
};


#line 463
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_1 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]])
{

#line 463
    thread KernelContext_0 kernelContext_0;

#line 463
    (&kernelContext_0)->draw_0 = draw_1;

#line 463
    (&kernelContext_0)->visible_instances_0 = visible_instances_1;

#line 463
    (&kernelContext_0)->instances_0 = instances_1;

#line 463
    (&kernelContext_0)->meshes_0 = meshes_1;

#line 463
    (&kernelContext_0)->vertices_0 = vertices_1;

#line 463
    (&kernelContext_0)->frame_0 = frame_1;

#line 463
    (&kernelContext_0)->materials_0 = materials_1;

#line 463
    (&kernelContext_0)->base_color_textures_0 = base_color_textures_1;

#line 463
    (&kernelContext_0)->base_color_sampler_0 = base_color_sampler_1;



    float3 normal_1 = normalize(_S1.world_normal_0);
    float3 to_light_0 = normalize(frame_1->light_direction_0.xyz);

#line 473
    GpuMaterial_natural_0 material_2 = materials_1[_S1.material_0];

#line 483
    float3 _S2 = float3(_S1.uv_0, float(material_2.base_color_texture_0));
    float4 albedo_0 = _S1.color_0 * float4(material_2.base_color_0)  * ((base_color_textures_1).sample((base_color_sampler_1), ((_S2)).xy, uint(((_S2)).z)));


    float _S3 = max(dot(normal_1, to_light_0), 0.0f);

#line 487
    pixelOutput_0 _S4 = { float4(albedo_0.xyz * (frame_1->ambient_0.xyz + frame_1->light_color_0.xyz * float3(_S3) ) + frame_1->light_color_0.xyz * float3((pow(max(dot(normal_1, normalize(to_light_0 + normalize(frame_1->camera_position_0.xyz - _S1.world_position_0))), 0.0f), 32.0f) * (step(0.0f, _S3) * _S3) * 0.34999999403953552f)) , albedo_0.w) };

#line 502
    return _S4;
}


#line 502
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float3 world_position_1 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
};


#line 382
struct VertexOutput_0
{
    float4 position_3;
    float3 world_position_2;
    float3 world_normal_2;
    float4 color_3;
    [[flat]] uint material_4;
    float2 uv_3;
};


#line 382
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]])
{

#line 382
    thread KernelContext_0 kernelContext_1;

#line 382
    (&kernelContext_1)->draw_0 = draw_2;

#line 382
    (&kernelContext_1)->visible_instances_0 = visible_instances_2;

#line 382
    (&kernelContext_1)->instances_0 = instances_2;

#line 382
    (&kernelContext_1)->meshes_0 = meshes_2;

#line 382
    (&kernelContext_1)->vertices_0 = vertices_2;

#line 382
    (&kernelContext_1)->frame_0 = frame_2;

#line 382
    (&kernelContext_1)->materials_0 = materials_2;

#line 382
    (&kernelContext_1)->base_color_textures_0 = base_color_textures_2;

#line 382
    (&kernelContext_1)->base_color_sampler_0 = base_color_sampler_2;

#line 422
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 427
    MeshVertex_natural_0 vertex_0 = vertices_2[index_0 + meshes_2[instance_0.mesh_0].base_vertex_0];

#line 427
    matrix<float,int(4),int(4)>  _S5 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S5)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_2 = world_0.xyz;

#line 438
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S5[int(0)].xyz, _S5[int(1)].xyz, _S5[int(2)].xyz))));
    (&output_1)->color_3 = float4(vertex_0.color_1) ;

#line 444
    (&output_1)->material_4 = instance_0.material_1;
    (&output_1)->uv_3 = (float4(vertex_0.uv_1) ).xy;

#line 445
    thread vertexMain_Result_0 _S6;

#line 445
    (&_S6)->position_2 = output_1.position_3;

#line 445
    (&_S6)->world_position_1 = output_1.world_position_2;

#line 445
    (&_S6)->world_normal_1 = output_1.world_normal_2;

#line 445
    (&_S6)->color_2 = output_1.color_3;

#line 445
    (&_S6)->material_3 = output_1.material_4;

#line 445
    (&_S6)->uv_2 = output_1.uv_3;

#line 445
    return _S6;
}

