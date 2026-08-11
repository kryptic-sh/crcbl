#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 269 "shaders/mesh_cluster.slang"
struct ClusterPayload_0
{
    uint cluster_0;
    uint instance_0;
};


#line 286
struct ClusterDrawConstants_0
{
    uint base_0;
    uint cluster_base_0;
    uint cluster_count_0;
    uint bucket_0;
};


#line 221
struct DrawIndexedArgs_0
{
    uint index_count_0;
    uint instance_count_0;
    uint first_index_0;
    int vertex_offset_0;
    uint first_instance_0;
};


#line 190
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


#line 544
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 544
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 160
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


#line 545
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 545
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 545
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 545
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 light_direction_0;
    float4 light_color_0;
    float4 ambient_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
};


#line 241
struct CullParams_0
{
    array<float4, int(6)> planes_0;
    uint instance_count_1;
    uint capacity_0;
};


#line 625
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
    CullParams_0 constant* cull_0;
    atomic<uint> device* cull_stats_0;
};


#line 429
uint group_is_live_0(uint3 group_0, KernelContext_0 thread* kernelContext_0)
{

    uint _S1 = group_0.y;
    uint _S2 = group_0.x;

#line 432
    return min(1U, max(kernelContext_0->draw_args_0[kernelContext_0->draw_0->bucket_0].instance_count_0, _S1) - _S1) * min(1U, max(kernelContext_0->draw_0->cluster_count_0, _S2) - _S2);
}


#line 407
uint corner_at_0(uint corner_0, KernelContext_0 thread* kernelContext_1)
{

    return (kernelContext_1->cluster_corners_0[corner_0 >> 2U] >> ((corner_0 & 3U) * 8U)) & 255U;
}


#line 393
struct VertexOutput_0
{
    float4 position_1 [[position]];
    float3 world_position_0 [[user(POSITION0)]];
    float3 world_normal_0 [[user(NORMAL0)]];
    float4 color_1 [[user(COLOR0)]];
    [[flat]] uint material_1 [[user(TEXCOORD0)]];
    float2 uv_1 [[user(TEXCOORD1)]];
};


#line 587
[[mesh]] void meshMain(uint3 lane_0 [[thread_position_in_threadgroup]], uint3 group_1 [[threadgroup_position_in_grid]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_1 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_1 [[buffer(10)]], Meshlet_0 device* clusters_1 [[buffer(7)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], uint device* cluster_vertices_1 [[buffer(8)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* cluster_corners_1 [[buffer(9)]], CullParams_0 constant* cull_1 [[buffer(11)]], atomic<uint> device* cull_stats_1 [[buffer(12)]])
{
    thread KernelContext_0 kernelContext_2;

#line 589
    (&kernelContext_2)->draw_0 = draw_1;

#line 589
    (&kernelContext_2)->draw_args_0 = draw_args_1;

#line 589
    (&kernelContext_2)->clusters_0 = clusters_1;

#line 589
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 589
    (&kernelContext_2)->instances_0 = instances_1;

#line 589
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 589
    (&kernelContext_2)->cluster_vertices_0 = cluster_vertices_1;

#line 589
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 589
    (&kernelContext_2)->frame_0 = frame_1;

#line 589
    (&kernelContext_2)->cluster_corners_0 = cluster_corners_1;

#line 589
    (&kernelContext_2)->cull_0 = cull_1;

#line 589
    (&kernelContext_2)->cull_stats_0 = cull_stats_1;

#line 589
    uint lane_1 = lane_0.x;

#line 589
    uint _S3 = group_is_live_0(group_1, &kernelContext_2);

#line 594
    uint _S4 = (&kernelContext_2)->draw_0->cluster_base_0 + group_1.x * _S3;

#line 594
    uint _S5 = group_1.y;

#line 594
    for(;;)
    {

#line 594
        Meshlet_0 cluster_1 = (&kernelContext_2)->clusters_0[_S4];

#line 594
        _slang_mesh.set_primitive_count((cluster_1.triangle_count_0 * _S3));

#line 594
        if(_S3 == 0U)
        {

#line 594
            break;
        }

#line 594
        GpuInstance_natural_0 instance_1 = (&kernelContext_2)->instances_0[(&kernelContext_2)->visible_instances_0[(&kernelContext_2)->draw_0->base_0 + _S5]];

#line 594
        GpuMesh_0 _S6 = (&kernelContext_2)->meshes_0[instance_1.mesh_0];

#line 594
        uint v_0 = lane_1;

#line 594
        for(;;)
        {

#line 594
            if(v_0 < (cluster_1.vertex_count_0))
            {
            }
            else
            {

#line 594
                break;
            }

#line 594
            MeshVertex_natural_0 vertex_0 = (&kernelContext_2)->vertices_0[(&kernelContext_2)->cluster_vertices_0[cluster_1.vertex_offset_1 + v_0] + _S6.base_vertex_0];

#line 594
            matrix<float,int(4),int(4)>  _S7 = matrix<float,int(4),int(4)> (instance_1.transform_0.data_0[int(0)][int(0)], instance_1.transform_0.data_0[int(1)][int(0)], instance_1.transform_0.data_0[int(2)][int(0)], instance_1.transform_0.data_0[int(3)][int(0)], instance_1.transform_0.data_0[int(0)][int(1)], instance_1.transform_0.data_0[int(1)][int(1)], instance_1.transform_0.data_0[int(2)][int(1)], instance_1.transform_0.data_0[int(3)][int(1)], instance_1.transform_0.data_0[int(0)][int(2)], instance_1.transform_0.data_0[int(1)][int(2)], instance_1.transform_0.data_0[int(2)][int(2)], instance_1.transform_0.data_0[int(3)][int(2)], instance_1.transform_0.data_0[int(0)][int(3)], instance_1.transform_0.data_0[int(1)][int(3)], instance_1.transform_0.data_0[int(2)][int(3)], instance_1.transform_0.data_0[int(3)][int(3)]);

#line 594
            float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S7)));

#line 594
            thread VertexOutput_0 output_0;

#line 594
            (&output_0)->position_1 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 594
            (&output_0)->world_position_0 = world_0.xyz;

#line 594
            (&output_0)->world_normal_0 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S7[int(0)].xyz, _S7[int(1)].xyz, _S7[int(2)].xyz))));

#line 594
            (&output_0)->color_1 = float4(vertex_0.color_0) ;

#line 594
            (&output_0)->material_1 = instance_1.material_0;

#line 594
            (&output_0)->uv_1 = (float4(vertex_0.uv_0) ).xy;

#line 594
            _slang_mesh.set_vertex(v_0,output_0);

#line 594
            v_0 = v_0 + 64U;

#line 594
        }

#line 594
        uint t_0 = lane_1;

#line 594
        for(;;)
        {

#line 594
            if(t_0 < (cluster_1.triangle_count_0))
            {
            }
            else
            {

#line 594
                break;
            }

#line 594
            uint corner_1 = cluster_1.triangle_offset_0 + t_0 * 3U;

#line 594
            uint _S8 = corner_at_0(corner_1, &kernelContext_2);

#line 594
            uint _S9 = corner_at_0(corner_1 + 1U, &kernelContext_2);

#line 594
            uint _S10 = corner_at_0(corner_1 + 2U, &kernelContext_2);

#line 594
            _slang_mesh.set_index(t_0*3+0,(uint3(_S8, _S9, _S10))[0]);
            _slang_mesh.set_index(t_0*3+1,(uint3(_S8, _S9, _S10))[1]);
            _slang_mesh.set_index(t_0*3+2,(uint3(_S8, _S9, _S10))[2]);
            ;

#line 594
            t_0 = t_0 + 64U;

#line 594
        }

#line 594
        break;
    }

#line 595
    return;
}


#line 476
uint cluster_survives_0(const Meshlet_0 thread* cluster_2, matrix<float,int(4),int(4)>  transform_1, KernelContext_0 thread* kernelContext_3)
{
    float3 center_0 = (((float4(cluster_2->center_x_0, cluster_2->center_y_0, cluster_2->center_z_0, 1.0f)) * (transform_1))).xyz;

#line 478
    float _S11 = cluster_2->radius_0;

#line 478
    uint plane_0 = 0U;


    for(;;)
    {

#line 481
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 481
            break;
        }

        float3 _S12 = kernelContext_3->cull_0->planes_0[plane_0].xyz;

#line 484
        if((dot(_S12, center_0) + kernelContext_3->cull_0->planes_0[plane_0].w) < (- _S11 * length(_S12)))
        {
            return 0U;
        }

#line 481
        plane_0 = plane_0 + 1U;

#line 481
    }

#line 490
    float3 axis_0 = (((float3(cluster_2->cone_axis_x_0, cluster_2->cone_axis_y_0, cluster_2->cone_axis_z_0)) * (matrix<float,int(3),int(3)> (transform_1[int(0)].xyz, transform_1[int(1)].xyz, transform_1[int(2)].xyz))));

    float3 to_center_0 = center_0 - kernelContext_3->frame_0->camera_position_0.xyz;
    float sine_0 = sqrt(max(0.0f, 1.0f - cluster_2->cone_cutoff_0 * cluster_2->cone_cutoff_0));

#line 493
    bool _S13;

    if((cluster_2->cone_cutoff_0) > 0.0f)
    {

#line 495
        _S13 = (dot(axis_0, to_center_0)) > (sine_0 * length(to_center_0) + _S11);

#line 495
    }
    else
    {

#line 495
        _S13 = false;

#line 495
    }

#line 494
    if(_S13)
    {

        return 0U;
    }

    return 1U;
}


#line 617
[[object]] void taskMain(uint3 group_2 [[threadgroup_position_in_grid]], ClusterPayload_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp, ClusterDrawConstants_0 constant* draw_2 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_2 [[buffer(10)]], Meshlet_0 device* clusters_2 [[buffer(7)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], uint device* cluster_vertices_2 [[buffer(8)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* cluster_corners_2 [[buffer(9)]], CullParams_0 constant* cull_2 [[buffer(11)]], atomic<uint> device* cull_stats_2 [[buffer(12)]])
{

#line 617
    thread KernelContext_0 kernelContext_4;

#line 617
    (&kernelContext_4)->draw_0 = draw_2;

#line 617
    (&kernelContext_4)->draw_args_0 = draw_args_2;

#line 617
    (&kernelContext_4)->clusters_0 = clusters_2;

#line 617
    (&kernelContext_4)->visible_instances_0 = visible_instances_2;

#line 617
    (&kernelContext_4)->instances_0 = instances_2;

#line 617
    (&kernelContext_4)->meshes_0 = meshes_2;

#line 617
    (&kernelContext_4)->cluster_vertices_0 = cluster_vertices_2;

#line 617
    (&kernelContext_4)->vertices_0 = vertices_2;

#line 617
    (&kernelContext_4)->frame_0 = frame_2;

#line 617
    (&kernelContext_4)->cluster_corners_0 = cluster_corners_2;

#line 617
    (&kernelContext_4)->cull_0 = cull_2;

#line 617
    (&kernelContext_4)->cull_stats_0 = cull_stats_2;

#line 617
    uint _S14 = group_is_live_0(group_2, &kernelContext_4);


    uint _S15 = group_2.x;
    uint _S16 = group_2.y;
    GpuInstance_natural_0 instance_2 = (&kernelContext_4)->instances_0[(&kernelContext_4)->visible_instances_0[(&kernelContext_4)->draw_0->base_0 + _S16 * _S14] * _S14];

#line 622
    matrix<float,int(4),int(4)>  _S17 = matrix<float,int(4),int(4)> (instance_2.transform_0.data_0[int(0)][int(0)], instance_2.transform_0.data_0[int(1)][int(0)], instance_2.transform_0.data_0[int(2)][int(0)], instance_2.transform_0.data_0[int(3)][int(0)], instance_2.transform_0.data_0[int(0)][int(1)], instance_2.transform_0.data_0[int(1)][int(1)], instance_2.transform_0.data_0[int(2)][int(1)], instance_2.transform_0.data_0[int(3)][int(1)], instance_2.transform_0.data_0[int(0)][int(2)], instance_2.transform_0.data_0[int(1)][int(2)], instance_2.transform_0.data_0[int(2)][int(2)], instance_2.transform_0.data_0[int(3)][int(2)], instance_2.transform_0.data_0[int(0)][int(3)], instance_2.transform_0.data_0[int(1)][int(3)], instance_2.transform_0.data_0[int(2)][int(3)], instance_2.transform_0.data_0[int(3)][int(3)]);

#line 622
    thread Meshlet_0 _S18 = (&kernelContext_4)->clusters_0[(&kernelContext_4)->draw_0->cluster_base_0 + _S15 * _S14];

#line 622
    uint _S19 = cluster_survives_0(&_S18, _S17, &kernelContext_4);

    uint keep_0 = _S14 * _S19;
    uint _S20 = atomic_fetch_add_explicit((&kernelContext_4)->cull_stats_0+1U, keep_0, memory_order_relaxed);

    thread ClusterPayload_0 payload_0;
    (&payload_0)->cluster_0 = _S15;
    (&payload_0)->instance_0 = _S16;
    *_slang_mesh_payload = *(&payload_0); _slang_mgp.set_threadgroups_per_grid(uint3((keep_0), (1U), (1U))); return;;
    return;
}


#line 642
[[mesh]] void amplifiedMeshMain(uint3 lane_2 [[thread_position_in_threadgroup]], const ClusterPayload_0 object_data* amplification_0 [[payload]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_3 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_3 [[buffer(10)]], Meshlet_0 device* clusters_3 [[buffer(7)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], uint device* cluster_vertices_3 [[buffer(8)]], MeshVertex_natural_0 device* vertices_3 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], uint device* cluster_corners_3 [[buffer(9)]], CullParams_0 constant* cull_3 [[buffer(11)]], atomic<uint> device* cull_stats_3 [[buffer(12)]])
{
    thread KernelContext_0 kernelContext_5;

#line 644
    (&kernelContext_5)->draw_0 = draw_3;

#line 644
    (&kernelContext_5)->draw_args_0 = draw_args_3;

#line 644
    (&kernelContext_5)->clusters_0 = clusters_3;

#line 644
    (&kernelContext_5)->visible_instances_0 = visible_instances_3;

#line 644
    (&kernelContext_5)->instances_0 = instances_3;

#line 644
    (&kernelContext_5)->meshes_0 = meshes_3;

#line 644
    (&kernelContext_5)->cluster_vertices_0 = cluster_vertices_3;

#line 644
    (&kernelContext_5)->vertices_0 = vertices_3;

#line 644
    (&kernelContext_5)->frame_0 = frame_3;

#line 644
    (&kernelContext_5)->cluster_corners_0 = cluster_corners_3;

#line 644
    (&kernelContext_5)->cull_0 = cull_3;

#line 644
    (&kernelContext_5)->cull_stats_0 = cull_stats_3;

#line 644
    uint lane_3 = lane_2.x;

#line 650
    uint _S21 = draw_3->cluster_base_0 + amplification_0->cluster_0;

#line 648
    uint _S22 = amplification_0->instance_0;

#line 648
    for(;;)
    {

#line 648
        Meshlet_0 cluster_3 = (&kernelContext_5)->clusters_0[_S21];

#line 648
        _slang_mesh.set_primitive_count((cluster_3.triangle_count_0));

#line 648
        GpuInstance_natural_0 instance_3 = (&kernelContext_5)->instances_0[(&kernelContext_5)->visible_instances_0[(&kernelContext_5)->draw_0->base_0 + _S22]];

#line 648
        GpuMesh_0 _S23 = (&kernelContext_5)->meshes_0[instance_3.mesh_0];

#line 648
        uint v_1 = lane_3;

#line 648
        for(;;)
        {

#line 648
            if(v_1 < (cluster_3.vertex_count_0))
            {
            }
            else
            {

#line 648
                break;
            }

#line 648
            MeshVertex_natural_0 vertex_1 = (&kernelContext_5)->vertices_0[(&kernelContext_5)->cluster_vertices_0[cluster_3.vertex_offset_1 + v_1] + _S23.base_vertex_0];

#line 648
            matrix<float,int(4),int(4)>  _S24 = matrix<float,int(4),int(4)> (instance_3.transform_0.data_0[int(0)][int(0)], instance_3.transform_0.data_0[int(1)][int(0)], instance_3.transform_0.data_0[int(2)][int(0)], instance_3.transform_0.data_0[int(3)][int(0)], instance_3.transform_0.data_0[int(0)][int(1)], instance_3.transform_0.data_0[int(1)][int(1)], instance_3.transform_0.data_0[int(2)][int(1)], instance_3.transform_0.data_0[int(3)][int(1)], instance_3.transform_0.data_0[int(0)][int(2)], instance_3.transform_0.data_0[int(1)][int(2)], instance_3.transform_0.data_0[int(2)][int(2)], instance_3.transform_0.data_0[int(3)][int(2)], instance_3.transform_0.data_0[int(0)][int(3)], instance_3.transform_0.data_0[int(1)][int(3)], instance_3.transform_0.data_0[int(2)][int(3)], instance_3.transform_0.data_0[int(3)][int(3)]);

#line 648
            float4 world_1 = (((float4((float4(vertex_1.position_0) ).xyz, 1.0f)) * (_S24)));

#line 648
            thread VertexOutput_0 output_1;

#line 648
            (&output_1)->position_1 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_5)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_5)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 648
            (&output_1)->world_position_0 = world_1.xyz;

#line 648
            (&output_1)->world_normal_0 = ((((float4(vertex_1.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S24[int(0)].xyz, _S24[int(1)].xyz, _S24[int(2)].xyz))));

#line 648
            (&output_1)->color_1 = float4(vertex_1.color_0) ;

#line 648
            (&output_1)->material_1 = instance_3.material_0;

#line 648
            (&output_1)->uv_1 = (float4(vertex_1.uv_0) ).xy;

#line 648
            _slang_mesh.set_vertex(v_1,output_1);

#line 648
            v_1 = v_1 + 64U;

#line 648
        }

#line 648
        uint t_1 = lane_3;

#line 648
        for(;;)
        {

#line 648
            if(t_1 < (cluster_3.triangle_count_0))
            {
            }
            else
            {

#line 648
                break;
            }

#line 648
            uint corner_2 = cluster_3.triangle_offset_0 + t_1 * 3U;

#line 648
            uint _S25 = corner_at_0(corner_2, &kernelContext_5);

#line 648
            uint _S26 = corner_at_0(corner_2 + 1U, &kernelContext_5);

#line 648
            uint _S27 = corner_at_0(corner_2 + 2U, &kernelContext_5);

#line 648
            _slang_mesh.set_index(t_1*3+0,(uint3(_S25, _S26, _S27))[0]);
            _slang_mesh.set_index(t_1*3+1,(uint3(_S25, _S26, _S27))[1]);
            _slang_mesh.set_index(t_1*3+2,(uint3(_S25, _S26, _S27))[2]);
            ;

#line 648
            t_1 = t_1 + 64U;

#line 648
        }

#line 648
        break;
    }

#line 655
    return;
}

