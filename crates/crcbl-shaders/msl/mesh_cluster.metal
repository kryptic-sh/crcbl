#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 409 "shaders/mesh_cluster.slang"
struct ClusterPayload_0
{
    uint cluster_0;
    uint instance_0;
};


#line 426
struct ClusterDrawConstants_0
{
    uint base_0;
    uint cluster_base_0;
    uint cluster_count_0;
    uint bucket_0;
    uint group_stride_0;
};


#line 361
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


#line 824
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 824
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


#line 337
struct ClusterSelect_0
{
    uint flags_1;
    uint vertex_base_0;
    uint producer_group_0;
    uint container_group_0;
};


#line 832
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 832
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 832
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


#line 381
struct CullParams_0
{
    array<float4, int(6)> planes_0;
    uint instance_count_1;
    uint capacity_0;
};


#line 933
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


#line 631
uint group_is_live_0(uint3 group_0, KernelContext_0 thread* kernelContext_0)
{

    uint _S1 = group_0.y;
    uint _S2 = group_0.x;

#line 634
    return min(1U, max(kernelContext_0->draw_args_0[kernelContext_0->draw_0->bucket_0].instance_count_0, _S1) - _S1) * min(1U, max(kernelContext_0->draw_0->cluster_count_0, _S2) - _S2);
}


#line 306
float3 lod_tint_0(uint level_0)
{
    switch(level_0 % 8U)
    {
    case 0U:
        {

#line 310
            return float3(0.89999997615814209f, 0.25f, 0.25f);
        }
    case 1U:
        {

#line 311
            return float3(0.94999998807907104f, 0.60000002384185791f, 0.20000000298023224f);
        }
    case 2U:
        {

#line 312
            return float3(0.89999997615814209f, 0.89999997615814209f, 0.25f);
        }
    case 3U:
        {

#line 313
            return float3(0.30000001192092896f, 0.85000002384185791f, 0.34999999403953552f);
        }
    case 4U:
        {

#line 314
            return float3(0.25f, 0.80000001192092896f, 0.85000002384185791f);
        }
    case 5U:
        {

#line 315
            return float3(0.30000001192092896f, 0.44999998807907104f, 0.94999998807907104f);
        }
    case 6U:
        {

#line 316
            return float3(0.64999997615814209f, 0.34999999403953552f, 0.89999997615814209f);
        }
    default:
        {

#line 317
            return float3(0.94999998807907104f, 0.44999998807907104f, 0.80000001192092896f);
        }
    }

#line 317
}


#line 609
uint corner_at_0(uint corner_0, KernelContext_0 thread* kernelContext_1)
{

    return (kernelContext_1->cluster_corners_0[corner_0 >> 2U] >> ((corner_0 & 3U) * 8U)) & 255U;
}


#line 595
struct VertexOutput_0
{
    float4 position_1 [[position]];
    float3 world_position_0 [[user(POSITION0)]];
    float3 world_normal_0 [[user(NORMAL0)]];
    float4 color_1 [[user(COLOR0)]];
    [[flat]] uint material_1 [[user(TEXCOORD0)]];
    float2 uv_1 [[user(TEXCOORD1)]];
};


#line 879
[[mesh]] void meshMain(uint3 lane_0 [[thread_position_in_threadgroup]], uint3 group_1 [[threadgroup_position_in_grid]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_1 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_1 [[buffer(10)]], Meshlet_0 device* clusters_1 [[buffer(7)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], ClusterSelect_0 device* cluster_select_1 [[buffer(13)]], uint device* cluster_vertices_1 [[buffer(8)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* cluster_corners_1 [[buffer(9)]], uint device* group_state_1 [[buffer(15)]], CullParams_0 constant* cull_1 [[buffer(11)]], atomic<uint> device* cull_stats_1 [[buffer(12)]], uint device* cluster_selection_1 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_2;

#line 881
    (&kernelContext_2)->draw_0 = draw_1;

#line 881
    (&kernelContext_2)->draw_args_0 = draw_args_1;

#line 881
    (&kernelContext_2)->clusters_0 = clusters_1;

#line 881
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 881
    (&kernelContext_2)->instances_0 = instances_1;

#line 881
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 881
    (&kernelContext_2)->cluster_select_0 = cluster_select_1;

#line 881
    (&kernelContext_2)->cluster_vertices_0 = cluster_vertices_1;

#line 881
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 881
    (&kernelContext_2)->frame_0 = frame_1;

#line 881
    (&kernelContext_2)->cluster_corners_0 = cluster_corners_1;

#line 881
    (&kernelContext_2)->group_state_0 = group_state_1;

#line 881
    (&kernelContext_2)->cull_0 = cull_1;

#line 881
    (&kernelContext_2)->cull_stats_0 = cull_stats_1;

#line 881
    (&kernelContext_2)->cluster_selection_0 = cluster_selection_1;

#line 881
    uint lane_1 = lane_0.x;

#line 881
    uint _S3 = group_is_live_0(group_1, &kernelContext_2);

#line 886
    uint _S4 = (&kernelContext_2)->draw_0->cluster_base_0 + group_1.x * _S3;

#line 886
    uint _S5 = group_1.y;

#line 886
    for(;;)
    {

#line 886
        Meshlet_0 cluster_1 = (&kernelContext_2)->clusters_0[_S4];

#line 886
        _slang_mesh.set_primitive_count((cluster_1.triangle_count_0 * _S3));

#line 886
        if(_S3 == 0U)
        {

#line 886
            break;
        }

#line 886
        GpuInstance_natural_0 instance_1 = (&kernelContext_2)->instances_0[(&kernelContext_2)->visible_instances_0[(&kernelContext_2)->draw_0->base_0 + _S5]];

#line 886
        GpuMesh_0 _S6 = (&kernelContext_2)->meshes_0[instance_1.mesh_0];

#line 886
        ClusterSelect_0 _S7 = (&kernelContext_2)->cluster_select_0[_S4];

#line 886
        uint v_0 = lane_1;

#line 886
        for(;;)
        {

#line 886
            if(v_0 < (cluster_1.vertex_count_0))
            {
            }
            else
            {

#line 886
                break;
            }

#line 886
            MeshVertex_natural_0 vertex_0 = (&kernelContext_2)->vertices_0[(&kernelContext_2)->cluster_vertices_0[cluster_1.vertex_offset_1 + v_0] + _S6.base_vertex_0 + _S7.vertex_base_0];

#line 886
            matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (instance_1.transform_0.data_0[int(0)][int(0)], instance_1.transform_0.data_0[int(1)][int(0)], instance_1.transform_0.data_0[int(2)][int(0)], instance_1.transform_0.data_0[int(3)][int(0)], instance_1.transform_0.data_0[int(0)][int(1)], instance_1.transform_0.data_0[int(1)][int(1)], instance_1.transform_0.data_0[int(2)][int(1)], instance_1.transform_0.data_0[int(3)][int(1)], instance_1.transform_0.data_0[int(0)][int(2)], instance_1.transform_0.data_0[int(1)][int(2)], instance_1.transform_0.data_0[int(2)][int(2)], instance_1.transform_0.data_0[int(3)][int(2)], instance_1.transform_0.data_0[int(0)][int(3)], instance_1.transform_0.data_0[int(1)][int(3)], instance_1.transform_0.data_0[int(2)][int(3)], instance_1.transform_0.data_0[int(3)][int(3)]);

#line 886
            float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S8)));

#line 886
            thread VertexOutput_0 output_0;

#line 886
            (&output_0)->position_1 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 886
            (&output_0)->world_position_0 = world_0.xyz;

#line 886
            (&output_0)->world_normal_0 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S8[int(0)].xyz, _S8[int(1)].xyz, _S8[int(2)].xyz))));

#line 886
            float4 _S9;

#line 886
            if(((&kernelContext_2)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 886
                _S9 = float4(lod_tint_0(((&kernelContext_2)->cluster_select_0[_S4].flags_1) >> 2U), 1.0f);

#line 886
            }
            else
            {

#line 886
                _S9 = float4(vertex_0.color_0) ;

#line 886
            }

#line 886
            (&output_0)->color_1 = _S9;

#line 886
            (&output_0)->material_1 = instance_1.material_0;

#line 886
            (&output_0)->uv_1 = (float4(vertex_0.uv_0) ).xy;

#line 886
            _slang_mesh.set_vertex(v_0,output_0);

#line 886
            v_0 = v_0 + 64U;

#line 886
        }

#line 886
        uint t_0 = lane_1;

#line 886
        for(;;)
        {

#line 886
            if(t_0 < (cluster_1.triangle_count_0))
            {
            }
            else
            {

#line 886
                break;
            }

#line 886
            uint corner_1 = cluster_1.triangle_offset_0 + t_0 * 3U;

#line 886
            uint _S10 = corner_at_0(corner_1, &kernelContext_2);

#line 886
            uint _S11 = corner_at_0(corner_1 + 1U, &kernelContext_2);

#line 886
            uint _S12 = corner_at_0(corner_1 + 2U, &kernelContext_2);

#line 886
            _slang_mesh.set_index(t_0*3+0,(uint3(_S10, _S11, _S12))[0]);
            _slang_mesh.set_index(t_0*3+1,(uint3(_S10, _S11, _S12))[1]);
            _slang_mesh.set_index(t_0*3+2,(uint3(_S10, _S11, _S12))[2]);
            ;

#line 886
            t_0 = t_0 + 64U;

#line 886
        }

#line 886
        break;
    }

#line 887
    return;
}


#line 771
uint cluster_is_selected_0(const ClusterSelect_0 thread* select_0, uint instance_index_0, KernelContext_0 thread* kernelContext_3)
{
    uint base_1 = instance_index_0 * kernelContext_3->draw_0->group_stride_0;

#line 773
    uint _S13 = select_0->flags_1;

#line 773
    bool producer_expanded_0;


    if(((select_0->flags_1) & 1U) != 0U)
    {

#line 776
        producer_expanded_0 = kernelContext_3->group_state_0[base_1 + select_0->producer_group_0] != 0U;

#line 776
    }
    else
    {

#line 776
        producer_expanded_0 = false;

#line 776
    }

#line 776
    bool container_expanded_0;

    if((_S13 & 2U) == 0U)
    {

#line 778
        container_expanded_0 = true;

#line 778
    }
    else
    {

#line 778
        container_expanded_0 = kernelContext_3->group_state_0[base_1 + select_0->container_group_0] != 0U;

#line 778
    }

    if(!producer_expanded_0)
    {

#line 780
        producer_expanded_0 = container_expanded_0;

#line 780
    }
    else
    {

#line 780
        producer_expanded_0 = false;

#line 780
    }

#line 780
    uint _S14;

#line 780
    if(producer_expanded_0)
    {

#line 780
        _S14 = 1U;

#line 780
    }
    else
    {

#line 780
        _S14 = 0U;

#line 780
    }

#line 780
    return _S14;
}


#line 658
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S15 = (((basis_0) * (transpose(basis_0))));

#line 660
    float bound_0 = 0.0f;

#line 660
    uint row_0 = 0U;

    for(;;)
    {

#line 662
        if(row_0 < 3U)
        {
        }
        else
        {

#line 662
            break;
        }
        float _S16 = max(bound_0, abs(_S15[row_0][int(0)]) + abs(_S15[row_0][int(1)]) + abs(_S15[row_0][int(2)]));

#line 662
        uint row_1 = row_0 + 1U;

#line 662
        bound_0 = _S16;

#line 662
        row_0 = row_1;

#line 662
    }



    return sqrt(bound_0);
}


#line 714
uint cluster_survives_0(const Meshlet_0 thread* cluster_2, matrix<float,int(4),int(4)>  transform_1, KernelContext_0 thread* kernelContext_4)
{
    matrix<float,int(3),int(3)>  _S17 = matrix<float,int(3),int(3)> (transform_1[int(0)].xyz, transform_1[int(1)].xyz, transform_1[int(2)].xyz);
    float3 center_0 = (((float4(cluster_2->center_x_0, cluster_2->center_y_0, cluster_2->center_z_0, 1.0f)) * (transform_1))).xyz;
    float radius_1 = cluster_2->radius_0 * max_stretch_0(_S17);

#line 718
    uint plane_0 = 0U;

    for(;;)
    {

#line 720
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 720
            break;
        }

        float3 _S18 = kernelContext_4->cull_0->planes_0[plane_0].xyz;

#line 723
        if((dot(_S18, center_0) + kernelContext_4->cull_0->planes_0[plane_0].w) < (- radius_1 * length(_S18)))
        {
            return 0U;
        }

#line 720
        plane_0 = plane_0 + 1U;

#line 720
    }

#line 737
    float3 axis_0 = (((float3(cluster_2->cone_axis_x_0, cluster_2->cone_axis_y_0, cluster_2->cone_axis_z_0)) * (_S17)));

    float axis_length_0 = length(axis_0);

#line 739
    float3 axis_1;
    if(axis_length_0 > 0.0f)
    {

#line 740
        axis_1 = axis_0 / float3(axis_length_0) ;

#line 740
    }
    else
    {

#line 740
        axis_1 = float3(0.0f, 0.0f, 0.0f);

#line 740
    }
    float3 to_center_0 = center_0 - kernelContext_4->frame_0->camera_position_0.xyz;
    float sine_0 = sqrt(max(0.0f, 1.0f - cluster_2->cone_cutoff_0 * cluster_2->cone_cutoff_0));

#line 742
    bool _S19;

    if((cluster_2->cone_cutoff_0) > 0.0f)
    {

#line 744
        _S19 = (dot(axis_1, to_center_0)) > (sine_0 * length(to_center_0) + radius_1);

#line 744
    }
    else
    {

#line 744
        _S19 = false;

#line 744
    }

#line 743
    if(_S19)
    {

        return 0U;
    }

    return 1U;
}


#line 909
[[object]] void taskMain(uint3 group_2 [[threadgroup_position_in_grid]], ClusterPayload_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp, ClusterDrawConstants_0 constant* draw_2 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_2 [[buffer(10)]], Meshlet_0 device* clusters_2 [[buffer(7)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], ClusterSelect_0 device* cluster_select_2 [[buffer(13)]], uint device* cluster_vertices_2 [[buffer(8)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* cluster_corners_2 [[buffer(9)]], uint device* group_state_2 [[buffer(15)]], CullParams_0 constant* cull_2 [[buffer(11)]], atomic<uint> device* cull_stats_2 [[buffer(12)]], uint device* cluster_selection_2 [[buffer(14)]])
{

#line 909
    thread KernelContext_0 kernelContext_5;

#line 909
    (&kernelContext_5)->draw_0 = draw_2;

#line 909
    (&kernelContext_5)->draw_args_0 = draw_args_2;

#line 909
    (&kernelContext_5)->clusters_0 = clusters_2;

#line 909
    (&kernelContext_5)->visible_instances_0 = visible_instances_2;

#line 909
    (&kernelContext_5)->instances_0 = instances_2;

#line 909
    (&kernelContext_5)->meshes_0 = meshes_2;

#line 909
    (&kernelContext_5)->cluster_select_0 = cluster_select_2;

#line 909
    (&kernelContext_5)->cluster_vertices_0 = cluster_vertices_2;

#line 909
    (&kernelContext_5)->vertices_0 = vertices_2;

#line 909
    (&kernelContext_5)->frame_0 = frame_2;

#line 909
    (&kernelContext_5)->cluster_corners_0 = cluster_corners_2;

#line 909
    (&kernelContext_5)->group_state_0 = group_state_2;

#line 909
    (&kernelContext_5)->cull_0 = cull_2;

#line 909
    (&kernelContext_5)->cull_stats_0 = cull_stats_2;

#line 909
    (&kernelContext_5)->cluster_selection_0 = cluster_selection_2;

#line 909
    uint _S20 = group_is_live_0(group_2, &kernelContext_5);


    uint _S21 = group_2.x;

#line 912
    uint _S22 = _S21 * _S20;

#line 912
    Meshlet_0 cluster_3 = (&kernelContext_5)->clusters_0[(&kernelContext_5)->draw_0->cluster_base_0 + _S22];
    uint _S23 = group_2.y;

#line 913
    uint instance_index_1 = (&kernelContext_5)->visible_instances_0[(&kernelContext_5)->draw_0->base_0 + _S23 * _S20] * _S20;
    GpuInstance_natural_0 instance_2 = (&kernelContext_5)->instances_0[instance_index_1];

    uint index_0 = (&kernelContext_5)->draw_0->cluster_base_0 + _S22;

#line 916
    thread ClusterSelect_0 _S24 = (&kernelContext_5)->cluster_select_0[index_0];

#line 916
    uint _S25 = cluster_is_selected_0(&_S24, instance_index_1, &kernelContext_5);

    uint _S26 = _S20 * _S25;

#line 918
    matrix<float,int(4),int(4)>  _S27 = matrix<float,int(4),int(4)> (instance_2.transform_0.data_0[int(0)][int(0)], instance_2.transform_0.data_0[int(1)][int(0)], instance_2.transform_0.data_0[int(2)][int(0)], instance_2.transform_0.data_0[int(3)][int(0)], instance_2.transform_0.data_0[int(0)][int(1)], instance_2.transform_0.data_0[int(1)][int(1)], instance_2.transform_0.data_0[int(2)][int(1)], instance_2.transform_0.data_0[int(3)][int(1)], instance_2.transform_0.data_0[int(0)][int(2)], instance_2.transform_0.data_0[int(1)][int(2)], instance_2.transform_0.data_0[int(2)][int(2)], instance_2.transform_0.data_0[int(3)][int(2)], instance_2.transform_0.data_0[int(0)][int(3)], instance_2.transform_0.data_0[int(1)][int(3)], instance_2.transform_0.data_0[int(2)][int(3)], instance_2.transform_0.data_0[int(3)][int(3)]);

#line 918
    thread Meshlet_0 _S28 = cluster_3;

#line 918
    uint _S29 = cluster_survives_0(&_S28, _S27, &kernelContext_5);

#line 918
    uint keep_0 = _S26 * _S29;
    uint _S30 = atomic_fetch_add_explicit((&kernelContext_5)->cull_stats_0+1U, keep_0, memory_order_relaxed);

#line 919
    bool _S31;

#line 931
    if(_S23 == 0U)
    {

#line 931
        _S31 = _S20 == 1U;

#line 931
    }
    else
    {

#line 931
        _S31 = false;

#line 931
    }

#line 931
    if(_S31)
    {
        *((&kernelContext_5)->cluster_selection_0+index_0) = _S25;

#line 931
    }

#line 936
    thread ClusterPayload_0 payload_0;
    (&payload_0)->cluster_0 = _S21;
    (&payload_0)->instance_0 = _S23;
    *_slang_mesh_payload = *(&payload_0); _slang_mgp.set_threadgroups_per_grid(uint3((keep_0), (1U), (1U))); return;;
    return;
}


#line 951
[[mesh]] void amplifiedMeshMain(uint3 lane_2 [[thread_position_in_threadgroup]], const ClusterPayload_0 object_data* amplification_0 [[payload]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_3 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_3 [[buffer(10)]], Meshlet_0 device* clusters_3 [[buffer(7)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], ClusterSelect_0 device* cluster_select_3 [[buffer(13)]], uint device* cluster_vertices_3 [[buffer(8)]], MeshVertex_natural_0 device* vertices_3 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], uint device* cluster_corners_3 [[buffer(9)]], uint device* group_state_3 [[buffer(15)]], CullParams_0 constant* cull_3 [[buffer(11)]], atomic<uint> device* cull_stats_3 [[buffer(12)]], uint device* cluster_selection_3 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_6;

#line 953
    (&kernelContext_6)->draw_0 = draw_3;

#line 953
    (&kernelContext_6)->draw_args_0 = draw_args_3;

#line 953
    (&kernelContext_6)->clusters_0 = clusters_3;

#line 953
    (&kernelContext_6)->visible_instances_0 = visible_instances_3;

#line 953
    (&kernelContext_6)->instances_0 = instances_3;

#line 953
    (&kernelContext_6)->meshes_0 = meshes_3;

#line 953
    (&kernelContext_6)->cluster_select_0 = cluster_select_3;

#line 953
    (&kernelContext_6)->cluster_vertices_0 = cluster_vertices_3;

#line 953
    (&kernelContext_6)->vertices_0 = vertices_3;

#line 953
    (&kernelContext_6)->frame_0 = frame_3;

#line 953
    (&kernelContext_6)->cluster_corners_0 = cluster_corners_3;

#line 953
    (&kernelContext_6)->group_state_0 = group_state_3;

#line 953
    (&kernelContext_6)->cull_0 = cull_3;

#line 953
    (&kernelContext_6)->cull_stats_0 = cull_stats_3;

#line 953
    (&kernelContext_6)->cluster_selection_0 = cluster_selection_3;

#line 953
    uint lane_3 = lane_2.x;

#line 959
    uint _S32 = draw_3->cluster_base_0 + amplification_0->cluster_0;

#line 957
    uint _S33 = amplification_0->instance_0;

#line 957
    for(;;)
    {

#line 957
        Meshlet_0 cluster_4 = (&kernelContext_6)->clusters_0[_S32];

#line 957
        _slang_mesh.set_primitive_count((cluster_4.triangle_count_0));

#line 957
        GpuInstance_natural_0 instance_3 = (&kernelContext_6)->instances_0[(&kernelContext_6)->visible_instances_0[(&kernelContext_6)->draw_0->base_0 + _S33]];

#line 957
        GpuMesh_0 _S34 = (&kernelContext_6)->meshes_0[instance_3.mesh_0];

#line 957
        ClusterSelect_0 _S35 = (&kernelContext_6)->cluster_select_0[_S32];

#line 957
        uint v_1 = lane_3;

#line 957
        for(;;)
        {

#line 957
            if(v_1 < (cluster_4.vertex_count_0))
            {
            }
            else
            {

#line 957
                break;
            }

#line 957
            MeshVertex_natural_0 vertex_1 = (&kernelContext_6)->vertices_0[(&kernelContext_6)->cluster_vertices_0[cluster_4.vertex_offset_1 + v_1] + _S34.base_vertex_0 + _S35.vertex_base_0];

#line 957
            matrix<float,int(4),int(4)>  _S36 = matrix<float,int(4),int(4)> (instance_3.transform_0.data_0[int(0)][int(0)], instance_3.transform_0.data_0[int(1)][int(0)], instance_3.transform_0.data_0[int(2)][int(0)], instance_3.transform_0.data_0[int(3)][int(0)], instance_3.transform_0.data_0[int(0)][int(1)], instance_3.transform_0.data_0[int(1)][int(1)], instance_3.transform_0.data_0[int(2)][int(1)], instance_3.transform_0.data_0[int(3)][int(1)], instance_3.transform_0.data_0[int(0)][int(2)], instance_3.transform_0.data_0[int(1)][int(2)], instance_3.transform_0.data_0[int(2)][int(2)], instance_3.transform_0.data_0[int(3)][int(2)], instance_3.transform_0.data_0[int(0)][int(3)], instance_3.transform_0.data_0[int(1)][int(3)], instance_3.transform_0.data_0[int(2)][int(3)], instance_3.transform_0.data_0[int(3)][int(3)]);

#line 957
            float4 world_1 = (((float4((float4(vertex_1.position_0) ).xyz, 1.0f)) * (_S36)));

#line 957
            thread VertexOutput_0 output_1;

#line 957
            (&output_1)->position_1 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 957
            (&output_1)->world_position_0 = world_1.xyz;

#line 957
            (&output_1)->world_normal_0 = ((((float4(vertex_1.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S36[int(0)].xyz, _S36[int(1)].xyz, _S36[int(2)].xyz))));

#line 957
            float4 _S37;

#line 957
            if(((&kernelContext_6)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 957
                _S37 = float4(lod_tint_0(((&kernelContext_6)->cluster_select_0[_S32].flags_1) >> 2U), 1.0f);

#line 957
            }
            else
            {

#line 957
                _S37 = float4(vertex_1.color_0) ;

#line 957
            }

#line 957
            (&output_1)->color_1 = _S37;

#line 957
            (&output_1)->material_1 = instance_3.material_0;

#line 957
            (&output_1)->uv_1 = (float4(vertex_1.uv_0) ).xy;

#line 957
            _slang_mesh.set_vertex(v_1,output_1);

#line 957
            v_1 = v_1 + 64U;

#line 957
        }

#line 957
        uint t_1 = lane_3;

#line 957
        for(;;)
        {

#line 957
            if(t_1 < (cluster_4.triangle_count_0))
            {
            }
            else
            {

#line 957
                break;
            }

#line 957
            uint corner_2 = cluster_4.triangle_offset_0 + t_1 * 3U;

#line 957
            uint _S38 = corner_at_0(corner_2, &kernelContext_6);

#line 957
            uint _S39 = corner_at_0(corner_2 + 1U, &kernelContext_6);

#line 957
            uint _S40 = corner_at_0(corner_2 + 2U, &kernelContext_6);

#line 957
            _slang_mesh.set_index(t_1*3+0,(uint3(_S38, _S39, _S40))[0]);
            _slang_mesh.set_index(t_1*3+1,(uint3(_S38, _S39, _S40))[1]);
            _slang_mesh.set_index(t_1*3+2,(uint3(_S38, _S39, _S40))[2]);
            ;

#line 957
            t_1 = t_1 + 64U;

#line 957
        }

#line 957
        break;
    }

#line 964
    return;
}

