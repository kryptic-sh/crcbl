#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 203 "shaders/mesh_cluster.slang"
struct ClusterDrawConstants_0
{
    uint base_0;
    uint cluster_base_0;
    uint cluster_count_0;
    uint bucket_0;
};


#line 187
struct DrawIndexedArgs_0
{
    uint index_count_0;
    uint instance_count_0;
    uint first_index_0;
    int vertex_offset_0;
    uint first_instance_0;
};


#line 157
struct Meshlet_0
{
    uint vertex_offset_1;
    uint vertex_count_0;
    uint triangle_offset_0;
    uint triangle_count_0;
    float center_x_0;
    float center_y_0;
    float center_z_0;
    float radius_0;
    float cone_axis_x_0;
    float cone_axis_y_0;
    float cone_axis_z_0;
    float cone_cutoff_0;
};


#line 355
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 355
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 127
struct GpuMesh_0
{
    uint base_vertex_0;
    uint base_index_0;
    uint index_count_1;
    float min_x_0;
    float min_y_0;
    float min_z_0;
    float max_x_0;
    float max_y_0;
    float max_z_0;
};


#line 356
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 356
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 356
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 light_direction_0;
    float4 light_color_0;
    float4 ambient_0;
};


#line 356
struct KernelContext_0
{
    ClusterDrawConstants_0 constant* draw_0;
    DrawIndexedArgs_0 device* draw_args_0;
    Meshlet_0 device* clusters_0;
    uint device* visible_instances_0;
    GpuInstance_natural_0 device* instances_0;
    GpuMesh_0 device* meshes_0;
    uint device* cluster_vertices_0;
    MeshVertex_natural_0 device* vertices_0;
    FrameUniforms_natural_0 constant* frame_0;
    uint device* cluster_corners_0;
};


#line 304
uint corner_at_0(uint corner_0, KernelContext_0 thread* kernelContext_0)
{

    return (kernelContext_0->cluster_corners_0[corner_0 >> 2U] >> ((corner_0 & 3U) * 8U)) & 255U;
}


#line 290
struct VertexOutput_0
{
    float4 position_1 [[position]];
    float3 world_position_0 [[user(POSITION0)]];
    float3 world_normal_0 [[user(NORMAL0)]];
    float4 color_1 [[user(COLOR0)]];
    [[flat]] uint material_1 [[user(TEXCOORD0)]];
    float2 uv_1 [[user(TEXCOORD1)]];
};


#line 313
[[mesh]] void meshMain(uint3 lane_0 [[thread_position_in_threadgroup]], uint3 group_0 [[threadgroup_position_in_grid]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_1 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_1 [[buffer(10)]], Meshlet_0 device* clusters_1 [[buffer(7)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], uint device* cluster_vertices_1 [[buffer(8)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* cluster_corners_1 [[buffer(9)]])
{
    thread KernelContext_0 kernelContext_1;

#line 315
    (&kernelContext_1)->draw_0 = draw_1;

#line 315
    (&kernelContext_1)->draw_args_0 = draw_args_1;

#line 315
    (&kernelContext_1)->clusters_0 = clusters_1;

#line 315
    (&kernelContext_1)->visible_instances_0 = visible_instances_1;

#line 315
    (&kernelContext_1)->instances_0 = instances_1;

#line 315
    (&kernelContext_1)->meshes_0 = meshes_1;

#line 315
    (&kernelContext_1)->cluster_vertices_0 = cluster_vertices_1;

#line 315
    (&kernelContext_1)->vertices_0 = vertices_1;

#line 315
    (&kernelContext_1)->frame_0 = frame_1;

#line 315
    (&kernelContext_1)->cluster_corners_0 = cluster_corners_1;

#line 315
    uint lane_1 = lane_0.x;

#line 338
    uint _S1 = group_0.y;
    uint _S2 = group_0.x;

#line 339
    uint active_0 = min(1U, max(draw_args_1[draw_1->bucket_0].instance_count_0, _S1) - _S1) * min(1U, max(draw_1->cluster_count_0, _S2) - _S2);

#line 345
    Meshlet_0 cluster_0 = clusters_1[draw_1->cluster_base_0 + _S2 * active_0];
    _slang_mesh.set_primitive_count((cluster_0.triangle_count_0 * active_0));
    if(active_0 == 0U)
    {
        return;
    }

#line 355
    GpuInstance_natural_0 instance_0 = (&kernelContext_1)->instances_0[(&kernelContext_1)->visible_instances_0[(&kernelContext_1)->draw_0->base_0 + _S1]];
    GpuMesh_0 _S3 = (&kernelContext_1)->meshes_0[instance_0.mesh_0];

#line 356
    uint v_0 = lane_1;

    for(;;)
    {

#line 358
        if(v_0 < (cluster_0.vertex_count_0))
        {
        }
        else
        {

#line 358
            break;
        }

        MeshVertex_natural_0 vertex_0 = (&kernelContext_1)->vertices_0[(&kernelContext_1)->cluster_vertices_0[cluster_0.vertex_offset_1 + v_0] + _S3.base_vertex_0];

#line 361
        matrix<float,int(4),int(4)>  _S4 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

        float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S4)));

        thread VertexOutput_0 output_0;
        (&output_0)->position_1 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_1)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_1)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
        (&output_0)->world_position_0 = world_0.xyz;



        (&output_0)->world_normal_0 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S4[int(0)].xyz, _S4[int(1)].xyz, _S4[int(2)].xyz))));
        (&output_0)->color_1 = float4(vertex_0.color_0) ;
        (&output_0)->material_1 = instance_0.material_0;
        (&output_0)->uv_1 = (float4(vertex_0.uv_0) ).xy;
        _slang_mesh.set_vertex(v_0,output_0);

#line 358
        v_0 = v_0 + 64U;

#line 358
    }

#line 358
    uint t_0 = lane_1;

#line 378
    for(;;)
    {

#line 378
        if(t_0 < (cluster_0.triangle_count_0))
        {
        }
        else
        {

#line 378
            break;
        }
        uint corner_1 = cluster_0.triangle_offset_0 + t_0 * 3U;

#line 380
        uint _S5 = corner_at_0(corner_1, &kernelContext_1);

#line 380
        uint _S6 = corner_at_0(corner_1 + 1U, &kernelContext_1);

#line 380
        uint _S7 = corner_at_0(corner_1 + 2U, &kernelContext_1);


        _slang_mesh.set_index(t_0*3+0,(uint3(_S5, _S6, _S7))[0]);
        _slang_mesh.set_index(t_0*3+1,(uint3(_S5, _S6, _S7))[1]);
        _slang_mesh.set_index(t_0*3+2,(uint3(_S5, _S6, _S7))[2]);
        ;

#line 378
        t_0 = t_0 + 64U;

#line 378
    }

#line 385
    return;
}

