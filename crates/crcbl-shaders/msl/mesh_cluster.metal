#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 362 "shaders/mesh_cluster.slang"
struct ClusterPayload_0
{
    uint cluster_0;
    uint instance_0;
};


#line 379
struct ClusterDrawConstants_0
{
    uint base_0;
    uint cluster_base_0;
    uint cluster_count_0;
    uint bucket_0;
    uint group_stride_0;
};


#line 314
struct DrawIndexedArgs_0
{
    uint index_count_0;
    uint instance_count_0;
    uint first_index_0;
    int vertex_offset_0;
    uint first_instance_0;
};


#line 241
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


#line 777
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 777
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 207
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


#line 290
struct ClusterSelect_0
{
    uint flags_1;
    uint vertex_base_0;
    uint producer_group_0;
    uint container_group_0;
};


#line 785
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 785
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 785
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 142
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E6_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(6)> data_3;
};


#line 142
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 ambient_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
    uint4 cluster_grid_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E6_0 light_view_proj_0;
    float4 probe_origin_0;
    float4 probe_inv_spacing_0;
    uint4 probe_counts_0;
};


#line 334
struct CullParams_0
{
    array<float4, int(6)> planes_0;
    uint instance_count_1;
    uint capacity_0;
};


#line 880
struct KernelContext_0
{
    ClusterDrawConstants_0 constant* draw_0;
    DrawIndexedArgs_0 device* draw_args_0;
    Meshlet_0 device* clusters_0;
    uint device* visible_instances_0;
    GpuInstance_natural_0 device* instances_0;
    GpuMesh_0 device* meshes_0;
    ClusterSelect_0 device* cluster_select_0;
    uint device* cluster_vertices_0;
    MeshVertex_natural_0 device* vertices_0;
    FrameUniforms_natural_0 constant* frame_0;
    uint device* cluster_corners_0;
    uint device* group_state_0;
    CullParams_0 constant* cull_0;
    atomic<uint> device* cull_stats_0;
    uint device* cluster_selection_0;
};


#line 584
uint group_is_live_0(uint3 group_0, KernelContext_0 thread* kernelContext_0)
{

    uint _S1 = group_0.y;
    uint _S2 = group_0.x;

#line 587
    return min(1U, max(kernelContext_0->draw_args_0[kernelContext_0->draw_0->bucket_0].instance_count_0, _S1) - _S1) * min(1U, max(kernelContext_0->draw_0->cluster_count_0, _S2) - _S2);
}


#line 562
uint corner_at_0(uint corner_0, KernelContext_0 thread* kernelContext_1)
{

    return (kernelContext_1->cluster_corners_0[corner_0 >> 2U] >> ((corner_0 & 3U) * 8U)) & 255U;
}


#line 548
struct VertexOutput_0
{
    float4 position_1 [[position]];
    float3 world_position_0 [[user(POSITION0)]];
    float3 world_normal_0 [[user(NORMAL0)]];
    float4 color_1 [[user(COLOR0)]];
    [[flat]] uint material_1 [[user(TEXCOORD0)]];
    float2 uv_1 [[user(TEXCOORD1)]];
};


#line 826
[[mesh]] void meshMain(uint3 lane_0 [[thread_position_in_threadgroup]], uint3 group_1 [[threadgroup_position_in_grid]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_1 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_1 [[buffer(10)]], Meshlet_0 device* clusters_1 [[buffer(7)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], ClusterSelect_0 device* cluster_select_1 [[buffer(13)]], uint device* cluster_vertices_1 [[buffer(8)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* cluster_corners_1 [[buffer(9)]], uint device* group_state_1 [[buffer(15)]], CullParams_0 constant* cull_1 [[buffer(11)]], atomic<uint> device* cull_stats_1 [[buffer(12)]], uint device* cluster_selection_1 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_2;

#line 828
    (&kernelContext_2)->draw_0 = draw_1;

#line 828
    (&kernelContext_2)->draw_args_0 = draw_args_1;

#line 828
    (&kernelContext_2)->clusters_0 = clusters_1;

#line 828
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 828
    (&kernelContext_2)->instances_0 = instances_1;

#line 828
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 828
    (&kernelContext_2)->cluster_select_0 = cluster_select_1;

#line 828
    (&kernelContext_2)->cluster_vertices_0 = cluster_vertices_1;

#line 828
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 828
    (&kernelContext_2)->frame_0 = frame_1;

#line 828
    (&kernelContext_2)->cluster_corners_0 = cluster_corners_1;

#line 828
    (&kernelContext_2)->group_state_0 = group_state_1;

#line 828
    (&kernelContext_2)->cull_0 = cull_1;

#line 828
    (&kernelContext_2)->cull_stats_0 = cull_stats_1;

#line 828
    (&kernelContext_2)->cluster_selection_0 = cluster_selection_1;

#line 828
    uint lane_1 = lane_0.x;

#line 828
    uint _S3 = group_is_live_0(group_1, &kernelContext_2);

#line 833
    uint _S4 = (&kernelContext_2)->draw_0->cluster_base_0 + group_1.x * _S3;

#line 833
    uint _S5 = group_1.y;

#line 833
    for(;;)
    {

#line 833
        Meshlet_0 cluster_1 = (&kernelContext_2)->clusters_0[_S4];

#line 833
        _slang_mesh.set_primitive_count((cluster_1.triangle_count_0 * _S3));

#line 833
        if(_S3 == 0U)
        {

#line 833
            break;
        }

#line 833
        GpuInstance_natural_0 instance_1 = (&kernelContext_2)->instances_0[(&kernelContext_2)->visible_instances_0[(&kernelContext_2)->draw_0->base_0 + _S5]];

#line 833
        GpuMesh_0 _S6 = (&kernelContext_2)->meshes_0[instance_1.mesh_0];

#line 833
        ClusterSelect_0 _S7 = (&kernelContext_2)->cluster_select_0[_S4];

#line 833
        uint v_0 = lane_1;

#line 833
        for(;;)
        {

#line 833
            if(v_0 < (cluster_1.vertex_count_0))
            {
            }
            else
            {

#line 833
                break;
            }

#line 833
            MeshVertex_natural_0 vertex_0 = (&kernelContext_2)->vertices_0[(&kernelContext_2)->cluster_vertices_0[cluster_1.vertex_offset_1 + v_0] + _S6.base_vertex_0 + _S7.vertex_base_0];

#line 833
            matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (instance_1.transform_0.data_0[int(0)][int(0)], instance_1.transform_0.data_0[int(1)][int(0)], instance_1.transform_0.data_0[int(2)][int(0)], instance_1.transform_0.data_0[int(3)][int(0)], instance_1.transform_0.data_0[int(0)][int(1)], instance_1.transform_0.data_0[int(1)][int(1)], instance_1.transform_0.data_0[int(2)][int(1)], instance_1.transform_0.data_0[int(3)][int(1)], instance_1.transform_0.data_0[int(0)][int(2)], instance_1.transform_0.data_0[int(1)][int(2)], instance_1.transform_0.data_0[int(2)][int(2)], instance_1.transform_0.data_0[int(3)][int(2)], instance_1.transform_0.data_0[int(0)][int(3)], instance_1.transform_0.data_0[int(1)][int(3)], instance_1.transform_0.data_0[int(2)][int(3)], instance_1.transform_0.data_0[int(3)][int(3)]);

#line 833
            float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S8)));

#line 833
            thread VertexOutput_0 output_0;

#line 833
            (&output_0)->position_1 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 833
            (&output_0)->world_position_0 = world_0.xyz;

#line 833
            (&output_0)->world_normal_0 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S8[int(0)].xyz, _S8[int(1)].xyz, _S8[int(2)].xyz))));

#line 833
            (&output_0)->color_1 = float4(vertex_0.color_0) ;

#line 833
            (&output_0)->material_1 = instance_1.material_0;

#line 833
            (&output_0)->uv_1 = (float4(vertex_0.uv_0) ).xy;

#line 833
            _slang_mesh.set_vertex(v_0,output_0);

#line 833
            v_0 = v_0 + 64U;

#line 833
        }

#line 833
        uint t_0 = lane_1;

#line 833
        for(;;)
        {

#line 833
            if(t_0 < (cluster_1.triangle_count_0))
            {
            }
            else
            {

#line 833
                break;
            }

#line 833
            uint corner_1 = cluster_1.triangle_offset_0 + t_0 * 3U;

#line 833
            uint _S9 = corner_at_0(corner_1, &kernelContext_2);

#line 833
            uint _S10 = corner_at_0(corner_1 + 1U, &kernelContext_2);

#line 833
            uint _S11 = corner_at_0(corner_1 + 2U, &kernelContext_2);

#line 833
            _slang_mesh.set_index(t_0*3+0,(uint3(_S9, _S10, _S11))[0]);
            _slang_mesh.set_index(t_0*3+1,(uint3(_S9, _S10, _S11))[1]);
            _slang_mesh.set_index(t_0*3+2,(uint3(_S9, _S10, _S11))[2]);
            ;

#line 833
            t_0 = t_0 + 64U;

#line 833
        }

#line 833
        break;
    }

#line 834
    return;
}


#line 724
uint cluster_is_selected_0(const ClusterSelect_0 thread* select_0, uint instance_index_0, KernelContext_0 thread* kernelContext_3)
{
    uint base_1 = instance_index_0 * kernelContext_3->draw_0->group_stride_0;

#line 726
    uint _S12 = select_0->flags_1;

#line 726
    bool producer_expanded_0;


    if(((select_0->flags_1) & 1U) != 0U)
    {

#line 729
        producer_expanded_0 = kernelContext_3->group_state_0[base_1 + select_0->producer_group_0] != 0U;

#line 729
    }
    else
    {

#line 729
        producer_expanded_0 = false;

#line 729
    }

#line 729
    bool container_expanded_0;

    if((_S12 & 2U) == 0U)
    {

#line 731
        container_expanded_0 = true;

#line 731
    }
    else
    {

#line 731
        container_expanded_0 = kernelContext_3->group_state_0[base_1 + select_0->container_group_0] != 0U;

#line 731
    }

    if(!producer_expanded_0)
    {

#line 733
        producer_expanded_0 = container_expanded_0;

#line 733
    }
    else
    {

#line 733
        producer_expanded_0 = false;

#line 733
    }

#line 733
    uint _S13;

#line 733
    if(producer_expanded_0)
    {

#line 733
        _S13 = 1U;

#line 733
    }
    else
    {

#line 733
        _S13 = 0U;

#line 733
    }

#line 733
    return _S13;
}


#line 611
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S14 = (((basis_0) * (transpose(basis_0))));

#line 613
    float bound_0 = 0.0f;

#line 613
    uint row_0 = 0U;

    for(;;)
    {

#line 615
        if(row_0 < 3U)
        {
        }
        else
        {

#line 615
            break;
        }
        float _S15 = max(bound_0, abs(_S14[row_0][int(0)]) + abs(_S14[row_0][int(1)]) + abs(_S14[row_0][int(2)]));

#line 615
        uint row_1 = row_0 + 1U;

#line 615
        bound_0 = _S15;

#line 615
        row_0 = row_1;

#line 615
    }



    return sqrt(bound_0);
}


#line 667
uint cluster_survives_0(const Meshlet_0 thread* cluster_2, matrix<float,int(4),int(4)>  transform_1, KernelContext_0 thread* kernelContext_4)
{
    matrix<float,int(3),int(3)>  _S16 = matrix<float,int(3),int(3)> (transform_1[int(0)].xyz, transform_1[int(1)].xyz, transform_1[int(2)].xyz);
    float3 center_0 = (((float4(cluster_2->center_x_0, cluster_2->center_y_0, cluster_2->center_z_0, 1.0f)) * (transform_1))).xyz;
    float radius_1 = cluster_2->radius_0 * max_stretch_0(_S16);

#line 671
    uint plane_0 = 0U;

    for(;;)
    {

#line 673
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 673
            break;
        }

        float3 _S17 = kernelContext_4->cull_0->planes_0[plane_0].xyz;

#line 676
        if((dot(_S17, center_0) + kernelContext_4->cull_0->planes_0[plane_0].w) < (- radius_1 * length(_S17)))
        {
            return 0U;
        }

#line 673
        plane_0 = plane_0 + 1U;

#line 673
    }

#line 690
    float3 axis_0 = (((float3(cluster_2->cone_axis_x_0, cluster_2->cone_axis_y_0, cluster_2->cone_axis_z_0)) * (_S16)));

    float axis_length_0 = length(axis_0);

#line 692
    float3 axis_1;
    if(axis_length_0 > 0.0f)
    {

#line 693
        axis_1 = axis_0 / float3(axis_length_0) ;

#line 693
    }
    else
    {

#line 693
        axis_1 = float3(0.0f, 0.0f, 0.0f);

#line 693
    }
    float3 to_center_0 = center_0 - kernelContext_4->frame_0->camera_position_0.xyz;
    float sine_0 = sqrt(max(0.0f, 1.0f - cluster_2->cone_cutoff_0 * cluster_2->cone_cutoff_0));

#line 695
    bool _S18;

    if((cluster_2->cone_cutoff_0) > 0.0f)
    {

#line 697
        _S18 = (dot(axis_1, to_center_0)) > (sine_0 * length(to_center_0) + radius_1);

#line 697
    }
    else
    {

#line 697
        _S18 = false;

#line 697
    }

#line 696
    if(_S18)
    {

        return 0U;
    }

    return 1U;
}


#line 856
[[object]] void taskMain(uint3 group_2 [[threadgroup_position_in_grid]], ClusterPayload_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp, ClusterDrawConstants_0 constant* draw_2 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_2 [[buffer(10)]], Meshlet_0 device* clusters_2 [[buffer(7)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], ClusterSelect_0 device* cluster_select_2 [[buffer(13)]], uint device* cluster_vertices_2 [[buffer(8)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* cluster_corners_2 [[buffer(9)]], uint device* group_state_2 [[buffer(15)]], CullParams_0 constant* cull_2 [[buffer(11)]], atomic<uint> device* cull_stats_2 [[buffer(12)]], uint device* cluster_selection_2 [[buffer(14)]])
{

#line 856
    thread KernelContext_0 kernelContext_5;

#line 856
    (&kernelContext_5)->draw_0 = draw_2;

#line 856
    (&kernelContext_5)->draw_args_0 = draw_args_2;

#line 856
    (&kernelContext_5)->clusters_0 = clusters_2;

#line 856
    (&kernelContext_5)->visible_instances_0 = visible_instances_2;

#line 856
    (&kernelContext_5)->instances_0 = instances_2;

#line 856
    (&kernelContext_5)->meshes_0 = meshes_2;

#line 856
    (&kernelContext_5)->cluster_select_0 = cluster_select_2;

#line 856
    (&kernelContext_5)->cluster_vertices_0 = cluster_vertices_2;

#line 856
    (&kernelContext_5)->vertices_0 = vertices_2;

#line 856
    (&kernelContext_5)->frame_0 = frame_2;

#line 856
    (&kernelContext_5)->cluster_corners_0 = cluster_corners_2;

#line 856
    (&kernelContext_5)->group_state_0 = group_state_2;

#line 856
    (&kernelContext_5)->cull_0 = cull_2;

#line 856
    (&kernelContext_5)->cull_stats_0 = cull_stats_2;

#line 856
    (&kernelContext_5)->cluster_selection_0 = cluster_selection_2;

#line 856
    uint _S19 = group_is_live_0(group_2, &kernelContext_5);


    uint _S20 = group_2.x;

#line 859
    uint _S21 = _S20 * _S19;

#line 859
    Meshlet_0 cluster_3 = (&kernelContext_5)->clusters_0[(&kernelContext_5)->draw_0->cluster_base_0 + _S21];
    uint _S22 = group_2.y;

#line 860
    uint instance_index_1 = (&kernelContext_5)->visible_instances_0[(&kernelContext_5)->draw_0->base_0 + _S22 * _S19] * _S19;
    GpuInstance_natural_0 instance_2 = (&kernelContext_5)->instances_0[instance_index_1];

    uint index_0 = (&kernelContext_5)->draw_0->cluster_base_0 + _S21;

#line 863
    thread ClusterSelect_0 _S23 = (&kernelContext_5)->cluster_select_0[index_0];

#line 863
    uint _S24 = cluster_is_selected_0(&_S23, instance_index_1, &kernelContext_5);

    uint _S25 = _S19 * _S24;

#line 865
    matrix<float,int(4),int(4)>  _S26 = matrix<float,int(4),int(4)> (instance_2.transform_0.data_0[int(0)][int(0)], instance_2.transform_0.data_0[int(1)][int(0)], instance_2.transform_0.data_0[int(2)][int(0)], instance_2.transform_0.data_0[int(3)][int(0)], instance_2.transform_0.data_0[int(0)][int(1)], instance_2.transform_0.data_0[int(1)][int(1)], instance_2.transform_0.data_0[int(2)][int(1)], instance_2.transform_0.data_0[int(3)][int(1)], instance_2.transform_0.data_0[int(0)][int(2)], instance_2.transform_0.data_0[int(1)][int(2)], instance_2.transform_0.data_0[int(2)][int(2)], instance_2.transform_0.data_0[int(3)][int(2)], instance_2.transform_0.data_0[int(0)][int(3)], instance_2.transform_0.data_0[int(1)][int(3)], instance_2.transform_0.data_0[int(2)][int(3)], instance_2.transform_0.data_0[int(3)][int(3)]);

#line 865
    thread Meshlet_0 _S27 = cluster_3;

#line 865
    uint _S28 = cluster_survives_0(&_S27, _S26, &kernelContext_5);

#line 865
    uint keep_0 = _S25 * _S28;
    uint _S29 = atomic_fetch_add_explicit((&kernelContext_5)->cull_stats_0+1U, keep_0, memory_order_relaxed);

#line 866
    bool _S30;

#line 878
    if(_S22 == 0U)
    {

#line 878
        _S30 = _S19 == 1U;

#line 878
    }
    else
    {

#line 878
        _S30 = false;

#line 878
    }

#line 878
    if(_S30)
    {
        *((&kernelContext_5)->cluster_selection_0+index_0) = _S24;

#line 878
    }

#line 883
    thread ClusterPayload_0 payload_0;
    (&payload_0)->cluster_0 = _S20;
    (&payload_0)->instance_0 = _S22;
    *_slang_mesh_payload = *(&payload_0); _slang_mgp.set_threadgroups_per_grid(uint3((keep_0), (1U), (1U))); return;;
    return;
}


#line 898
[[mesh]] void amplifiedMeshMain(uint3 lane_2 [[thread_position_in_threadgroup]], const ClusterPayload_0 object_data* amplification_0 [[payload]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_3 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_3 [[buffer(10)]], Meshlet_0 device* clusters_3 [[buffer(7)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], ClusterSelect_0 device* cluster_select_3 [[buffer(13)]], uint device* cluster_vertices_3 [[buffer(8)]], MeshVertex_natural_0 device* vertices_3 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], uint device* cluster_corners_3 [[buffer(9)]], uint device* group_state_3 [[buffer(15)]], CullParams_0 constant* cull_3 [[buffer(11)]], atomic<uint> device* cull_stats_3 [[buffer(12)]], uint device* cluster_selection_3 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_6;

#line 900
    (&kernelContext_6)->draw_0 = draw_3;

#line 900
    (&kernelContext_6)->draw_args_0 = draw_args_3;

#line 900
    (&kernelContext_6)->clusters_0 = clusters_3;

#line 900
    (&kernelContext_6)->visible_instances_0 = visible_instances_3;

#line 900
    (&kernelContext_6)->instances_0 = instances_3;

#line 900
    (&kernelContext_6)->meshes_0 = meshes_3;

#line 900
    (&kernelContext_6)->cluster_select_0 = cluster_select_3;

#line 900
    (&kernelContext_6)->cluster_vertices_0 = cluster_vertices_3;

#line 900
    (&kernelContext_6)->vertices_0 = vertices_3;

#line 900
    (&kernelContext_6)->frame_0 = frame_3;

#line 900
    (&kernelContext_6)->cluster_corners_0 = cluster_corners_3;

#line 900
    (&kernelContext_6)->group_state_0 = group_state_3;

#line 900
    (&kernelContext_6)->cull_0 = cull_3;

#line 900
    (&kernelContext_6)->cull_stats_0 = cull_stats_3;

#line 900
    (&kernelContext_6)->cluster_selection_0 = cluster_selection_3;

#line 900
    uint lane_3 = lane_2.x;

#line 906
    uint _S31 = draw_3->cluster_base_0 + amplification_0->cluster_0;

#line 904
    uint _S32 = amplification_0->instance_0;

#line 904
    for(;;)
    {

#line 904
        Meshlet_0 cluster_4 = (&kernelContext_6)->clusters_0[_S31];

#line 904
        _slang_mesh.set_primitive_count((cluster_4.triangle_count_0));

#line 904
        GpuInstance_natural_0 instance_3 = (&kernelContext_6)->instances_0[(&kernelContext_6)->visible_instances_0[(&kernelContext_6)->draw_0->base_0 + _S32]];

#line 904
        GpuMesh_0 _S33 = (&kernelContext_6)->meshes_0[instance_3.mesh_0];

#line 904
        ClusterSelect_0 _S34 = (&kernelContext_6)->cluster_select_0[_S31];

#line 904
        uint v_1 = lane_3;

#line 904
        for(;;)
        {

#line 904
            if(v_1 < (cluster_4.vertex_count_0))
            {
            }
            else
            {

#line 904
                break;
            }

#line 904
            MeshVertex_natural_0 vertex_1 = (&kernelContext_6)->vertices_0[(&kernelContext_6)->cluster_vertices_0[cluster_4.vertex_offset_1 + v_1] + _S33.base_vertex_0 + _S34.vertex_base_0];

#line 904
            matrix<float,int(4),int(4)>  _S35 = matrix<float,int(4),int(4)> (instance_3.transform_0.data_0[int(0)][int(0)], instance_3.transform_0.data_0[int(1)][int(0)], instance_3.transform_0.data_0[int(2)][int(0)], instance_3.transform_0.data_0[int(3)][int(0)], instance_3.transform_0.data_0[int(0)][int(1)], instance_3.transform_0.data_0[int(1)][int(1)], instance_3.transform_0.data_0[int(2)][int(1)], instance_3.transform_0.data_0[int(3)][int(1)], instance_3.transform_0.data_0[int(0)][int(2)], instance_3.transform_0.data_0[int(1)][int(2)], instance_3.transform_0.data_0[int(2)][int(2)], instance_3.transform_0.data_0[int(3)][int(2)], instance_3.transform_0.data_0[int(0)][int(3)], instance_3.transform_0.data_0[int(1)][int(3)], instance_3.transform_0.data_0[int(2)][int(3)], instance_3.transform_0.data_0[int(3)][int(3)]);

#line 904
            float4 world_1 = (((float4((float4(vertex_1.position_0) ).xyz, 1.0f)) * (_S35)));

#line 904
            thread VertexOutput_0 output_1;

#line 904
            (&output_1)->position_1 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 904
            (&output_1)->world_position_0 = world_1.xyz;

#line 904
            (&output_1)->world_normal_0 = ((((float4(vertex_1.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S35[int(0)].xyz, _S35[int(1)].xyz, _S35[int(2)].xyz))));

#line 904
            (&output_1)->color_1 = float4(vertex_1.color_0) ;

#line 904
            (&output_1)->material_1 = instance_3.material_0;

#line 904
            (&output_1)->uv_1 = (float4(vertex_1.uv_0) ).xy;

#line 904
            _slang_mesh.set_vertex(v_1,output_1);

#line 904
            v_1 = v_1 + 64U;

#line 904
        }

#line 904
        uint t_1 = lane_3;

#line 904
        for(;;)
        {

#line 904
            if(t_1 < (cluster_4.triangle_count_0))
            {
            }
            else
            {

#line 904
                break;
            }

#line 904
            uint corner_2 = cluster_4.triangle_offset_0 + t_1 * 3U;

#line 904
            uint _S36 = corner_at_0(corner_2, &kernelContext_6);

#line 904
            uint _S37 = corner_at_0(corner_2 + 1U, &kernelContext_6);

#line 904
            uint _S38 = corner_at_0(corner_2 + 2U, &kernelContext_6);

#line 904
            _slang_mesh.set_index(t_1*3+0,(uint3(_S36, _S37, _S38))[0]);
            _slang_mesh.set_index(t_1*3+1,(uint3(_S36, _S37, _S38))[1]);
            _slang_mesh.set_index(t_1*3+2,(uint3(_S36, _S37, _S38))[2]);
            ;

#line 904
            t_1 = t_1 + 64U;

#line 904
        }

#line 904
        break;
    }

#line 911
    return;
}

