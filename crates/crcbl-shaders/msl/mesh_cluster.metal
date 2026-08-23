#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 596 "shaders/mesh_cluster.slang"
struct ClusterPayload_0
{
    uint cluster_0;
    uint instance_0;
};


#line 613
struct ClusterDrawConstants_0
{
    uint base_0;
    uint cluster_base_0;
    uint cluster_count_0;
    uint bucket_0;
    uint group_stride_0;
    uint level_groups_at_0;
};


#line 535
struct DrawIndexedArgs_0
{
    uint index_count_0;
    uint instance_count_0;
    uint first_index_0;
    int vertex_offset_0;
    uint first_instance_0;
};


#line 272
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


#line 1174
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1174
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 238
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


#line 1175
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1175
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 142
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E7_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(7)> data_3;
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
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E7_0 light_view_proj_0;
    float4 probe_origin_0;
    float4 probe_inv_spacing_0;
    uint4 probe_counts_0;
    float4 lod_params_0;
};


#line 511
struct ClusterSelect_0
{
    uint flags_1;
    uint vertex_base_0;
    uint producer_group_0;
    uint container_group_0;
};


#line 934
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 555
struct CullParams_0
{
    array<float4, int(6)> planes_0;
    uint instance_count_1;
    uint capacity_0;
};


#line 1321
struct KernelContext_0
{
    ClusterDrawConstants_0 constant* draw_0;
    DrawIndexedArgs_0 device* draw_args_0;
    Meshlet_0 device* clusters_0;
    uint device* visible_instances_0;
    GpuInstance_natural_0 device* instances_0;
    GpuMesh_0 device* meshes_0;
    FrameUniforms_natural_0 constant* frame_0;
    ClusterSelect_0 device* cluster_select_0;
    uint device* tables_0;
    uint device* cluster_vertices_0;
    MeshVertex_natural_0 device* vertices_0;
    uint device* cluster_corners_0;
    uint device* group_state_0;
    CullParams_0 constant* cull_0;
    atomic<uint> device* cull_stats_0;
    uint device* cluster_selection_0;
};


#line 963
uint group_is_live_0(uint3 group_0, KernelContext_0 thread* kernelContext_0)
{

    uint _S1 = group_0.y;
    uint _S2 = group_0.x;

#line 966
    return min(1U, max(kernelContext_0->draw_args_0[kernelContext_0->draw_0->bucket_0].instance_count_0, _S1) - _S1) * min(1U, max(kernelContext_0->draw_0->cluster_count_0, _S2) - _S2);
}


#line 658
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_1;
    float center_y_1;
    float center_z_1;
    float radius_1;
};


#line 897
LevelGroup_0 level_group_at_0(uint group_1, KernelContext_0 thread* kernelContext_1)
{
    uint at_0 = kernelContext_1->draw_0->level_groups_at_0 + group_1 * 6U;
    thread LevelGroup_0 record_0;
    (&record_0)->level_0 = kernelContext_1->tables_0[at_0];
    (&record_0)->error_0 = (as_type<float>((kernelContext_1->tables_0[at_0 + 1U])));
    (&record_0)->center_x_1 = (as_type<float>((kernelContext_1->tables_0[at_0 + 2U])));
    (&record_0)->center_y_1 = (as_type<float>((kernelContext_1->tables_0[at_0 + 3U])));
    (&record_0)->center_z_1 = (as_type<float>((kernelContext_1->tables_0[at_0 + 4U])));
    (&record_0)->radius_1 = (as_type<float>((kernelContext_1->tables_0[at_0 + 5U])));
    return record_0;
}


#line 455
float projected_error_0(float error_1, float3 center_0, float radius_2, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S3 = delta_0.x;

#line 458
    float _S4 = delta_0.y;

#line 458
    float _S5 = delta_0.z;
    float distance_0 = sqrt(_S3 * _S3 + _S4 * _S4 + _S5 * _S5) - radius_2;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 414
float3 heat_tint_0(float projected_0, float expand_0, float hold_0)
{
    float _S6 = max(expand_0, 9.99999997475242708e-07f);
    float t_0 = projected_0 / _S6;
    if(t_0 >= 1.0f)
    {
        return float3(1.0f, 1.0f, 1.0f);
    }



    float band_0 = clamp(hold_0 / _S6, 0.0f, 1.0f);
    if(t_0 >= band_0)
    {
        return mix(float3(0.85000002384185791f, 0.44999998807907104f, 0.10000000149011612f), float3(0.97000002861022949f, 0.85000002384185791f, 0.20000000298023224f), float3(saturate((t_0 - band_0) / max(1.0f - band_0, 9.99999997475242708e-07f))) );
    }

    return mix(float3(0.07999999821186066f, 0.10000000149011612f, 0.34999999403953552f), float3(0.10000000149011612f, 0.55000001192092896f, 0.60000002384185791f), float3(saturate(t_0 / max(band_0, 9.99999997475242708e-07f))) );
}


#line 932
float3 cluster_heat_0(uint cluster_index_0, matrix<float,int(4),int(4)>  transform_1, KernelContext_0 thread* kernelContext_2)
{
    ClusterSelect_0 select_0 = kernelContext_2->cluster_select_0[cluster_index_0];

#line 934
    float projected_1;

    if(((select_0.flags_1) & 1U) != 0U)
    {

#line 936
        LevelGroup_0 _S7 = level_group_at_0(select_0.producer_group_0, kernelContext_2);

#line 936
        projected_1 = projected_error_0(_S7.error_0, (((float4(_S7.center_x_1, _S7.center_y_1, _S7.center_z_1, 1.0f)) * (transform_1))).xyz, _S7.radius_1, kernelContext_2->frame_0->camera_position_0.xyz, kernelContext_2->frame_0->lod_params_0.x);

#line 936
    }
    else
    {

#line 936
        projected_1 = 0.0f;

#line 936
    }

#line 944
    return heat_tint_0(projected_1, kernelContext_2->frame_0->lod_params_0.y, kernelContext_2->frame_0->lod_params_0.z);
}


#line 480
float3 lod_tint_0(uint level_1)
{
    switch(level_1 % 8U)
    {
    case 0U:
        {

#line 484
            return float3(0.89999997615814209f, 0.25f, 0.25f);
        }
    case 1U:
        {

#line 485
            return float3(0.94999998807907104f, 0.60000002384185791f, 0.20000000298023224f);
        }
    case 2U:
        {

#line 486
            return float3(0.89999997615814209f, 0.89999997615814209f, 0.25f);
        }
    case 3U:
        {

#line 487
            return float3(0.30000001192092896f, 0.85000002384185791f, 0.34999999403953552f);
        }
    case 4U:
        {

#line 488
            return float3(0.25f, 0.80000001192092896f, 0.85000002384185791f);
        }
    case 5U:
        {

#line 489
            return float3(0.30000001192092896f, 0.44999998807907104f, 0.94999998807907104f);
        }
    case 6U:
        {

#line 490
            return float3(0.64999997615814209f, 0.34999999403953552f, 0.89999997615814209f);
        }
    default:
        {

#line 491
            return float3(0.94999998807907104f, 0.44999998807907104f, 0.80000001192092896f);
        }
    }

#line 491
}


#line 883
uint corner_at_0(uint corner_0, KernelContext_0 thread* kernelContext_3)
{

    return (kernelContext_3->cluster_corners_0[corner_0 >> 2U] >> ((corner_0 & 3U) * 8U)) & 255U;
}


#line 869
struct VertexOutput_0
{
    float4 position_1 [[position]];
    float3 world_position_0 [[user(POSITION0)]];
    float3 world_normal_0 [[user(NORMAL0)]];
    float4 color_1 [[user(COLOR0)]];
    [[flat]] uint material_1 [[user(TEXCOORD0)]];
    float2 uv_1 [[user(TEXCOORD1)]];
};


#line 1246
[[mesh]] void meshMain(uint3 lane_0 [[thread_position_in_threadgroup]], uint3 group_2 [[threadgroup_position_in_grid]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_1 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_1 [[buffer(10)]], Meshlet_0 device* clusters_1 [[buffer(7)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], ClusterSelect_0 device* cluster_select_1 [[buffer(13)]], uint device* tables_1 [[buffer(19)]], uint device* cluster_vertices_1 [[buffer(8)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], uint device* cluster_corners_1 [[buffer(9)]], uint device* group_state_1 [[buffer(15)]], CullParams_0 constant* cull_1 [[buffer(11)]], atomic<uint> device* cull_stats_1 [[buffer(12)]], uint device* cluster_selection_1 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_4;

#line 1248
    (&kernelContext_4)->draw_0 = draw_1;

#line 1248
    (&kernelContext_4)->draw_args_0 = draw_args_1;

#line 1248
    (&kernelContext_4)->clusters_0 = clusters_1;

#line 1248
    (&kernelContext_4)->visible_instances_0 = visible_instances_1;

#line 1248
    (&kernelContext_4)->instances_0 = instances_1;

#line 1248
    (&kernelContext_4)->meshes_0 = meshes_1;

#line 1248
    (&kernelContext_4)->frame_0 = frame_1;

#line 1248
    (&kernelContext_4)->cluster_select_0 = cluster_select_1;

#line 1248
    (&kernelContext_4)->tables_0 = tables_1;

#line 1248
    (&kernelContext_4)->cluster_vertices_0 = cluster_vertices_1;

#line 1248
    (&kernelContext_4)->vertices_0 = vertices_1;

#line 1248
    (&kernelContext_4)->cluster_corners_0 = cluster_corners_1;

#line 1248
    (&kernelContext_4)->group_state_0 = group_state_1;

#line 1248
    (&kernelContext_4)->cull_0 = cull_1;

#line 1248
    (&kernelContext_4)->cull_stats_0 = cull_stats_1;

#line 1248
    (&kernelContext_4)->cluster_selection_0 = cluster_selection_1;

#line 1248
    uint lane_1 = lane_0.x;

#line 1248
    uint _S8 = group_is_live_0(group_2, &kernelContext_4);

#line 1253
    uint _S9 = (&kernelContext_4)->draw_0->cluster_base_0 + group_2.x * _S8;

#line 1253
    uint _S10 = group_2.y;

#line 1253
    for(;;)
    {

#line 1253
        Meshlet_0 cluster_1 = (&kernelContext_4)->clusters_0[_S9];

#line 1253
        _slang_mesh.set_primitive_count((cluster_1.triangle_count_0 * _S8));

#line 1253
        if(_S8 == 0U)
        {

#line 1253
            break;
        }

#line 1253
        GpuInstance_natural_0 instance_1 = (&kernelContext_4)->instances_0[(&kernelContext_4)->visible_instances_0[(&kernelContext_4)->draw_0->base_0 + _S10]];

#line 1253
        GpuMesh_0 _S11 = (&kernelContext_4)->meshes_0[instance_1.mesh_0];

#line 1253
        float4 _S12 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 1253
        float4 overlay_0;

#line 1253
        if(((&kernelContext_4)->frame_0->ambient_0.w) >= 2.5f)
        {

#line 1253
            float3 _S13 = cluster_heat_0(_S9, matrix<float,int(4),int(4)> (instance_1.transform_0.data_0[int(0)][int(0)], instance_1.transform_0.data_0[int(1)][int(0)], instance_1.transform_0.data_0[int(2)][int(0)], instance_1.transform_0.data_0[int(3)][int(0)], instance_1.transform_0.data_0[int(0)][int(1)], instance_1.transform_0.data_0[int(1)][int(1)], instance_1.transform_0.data_0[int(2)][int(1)], instance_1.transform_0.data_0[int(3)][int(1)], instance_1.transform_0.data_0[int(0)][int(2)], instance_1.transform_0.data_0[int(1)][int(2)], instance_1.transform_0.data_0[int(2)][int(2)], instance_1.transform_0.data_0[int(3)][int(2)], instance_1.transform_0.data_0[int(0)][int(3)], instance_1.transform_0.data_0[int(1)][int(3)], instance_1.transform_0.data_0[int(2)][int(3)], instance_1.transform_0.data_0[int(3)][int(3)]), &kernelContext_4);

#line 1253
            overlay_0 = float4(_S13, 1.0f);

#line 1253
        }
        else
        {

#line 1253
            if(((&kernelContext_4)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1253
                overlay_0 = float4(lod_tint_0(((&kernelContext_4)->cluster_select_0[_S9].flags_1) >> 2U), 1.0f);

#line 1253
            }
            else
            {

#line 1253
                overlay_0 = _S12;

#line 1253
            }

#line 1253
        }

#line 1253
        ClusterSelect_0 _S14 = (&kernelContext_4)->cluster_select_0[_S9];

#line 1253
        uint v_0 = lane_1;

#line 1253
        for(;;)
        {

#line 1253
            if(v_0 < (cluster_1.vertex_count_0))
            {
            }
            else
            {

#line 1253
                break;
            }

#line 1253
            MeshVertex_natural_0 vertex_0 = (&kernelContext_4)->vertices_0[(&kernelContext_4)->cluster_vertices_0[cluster_1.vertex_offset_1 + v_0] + _S11.base_vertex_0 + _S14.vertex_base_0];

#line 1253
            matrix<float,int(4),int(4)>  _S15 = matrix<float,int(4),int(4)> (instance_1.transform_0.data_0[int(0)][int(0)], instance_1.transform_0.data_0[int(1)][int(0)], instance_1.transform_0.data_0[int(2)][int(0)], instance_1.transform_0.data_0[int(3)][int(0)], instance_1.transform_0.data_0[int(0)][int(1)], instance_1.transform_0.data_0[int(1)][int(1)], instance_1.transform_0.data_0[int(2)][int(1)], instance_1.transform_0.data_0[int(3)][int(1)], instance_1.transform_0.data_0[int(0)][int(2)], instance_1.transform_0.data_0[int(1)][int(2)], instance_1.transform_0.data_0[int(2)][int(2)], instance_1.transform_0.data_0[int(3)][int(2)], instance_1.transform_0.data_0[int(0)][int(3)], instance_1.transform_0.data_0[int(1)][int(3)], instance_1.transform_0.data_0[int(2)][int(3)], instance_1.transform_0.data_0[int(3)][int(3)]);

#line 1253
            float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S15)));

#line 1253
            thread VertexOutput_0 output_0;

#line 1253
            (&output_0)->position_1 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_4)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 1253
            (&output_0)->world_position_0 = world_0.xyz;

#line 1253
            (&output_0)->world_normal_0 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S15[int(0)].xyz, _S15[int(1)].xyz, _S15[int(2)].xyz))));

#line 1253
            float4 _S16;

#line 1253
            if(((&kernelContext_4)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1253
                _S16 = overlay_0;

#line 1253
            }
            else
            {

#line 1253
                _S16 = float4(vertex_0.color_0) ;

#line 1253
            }

#line 1253
            (&output_0)->color_1 = _S16;

#line 1253
            (&output_0)->material_1 = instance_1.material_0;

#line 1253
            (&output_0)->uv_1 = (float4(vertex_0.uv_0) ).xy;

#line 1253
            _slang_mesh.set_vertex(v_0,output_0);

#line 1253
            v_0 = v_0 + 64U;

#line 1253
        }

#line 1253
        uint t_1 = lane_1;

#line 1253
        for(;;)
        {

#line 1253
            if(t_1 < (cluster_1.triangle_count_0))
            {
            }
            else
            {

#line 1253
                break;
            }

#line 1253
            uint corner_1 = cluster_1.triangle_offset_0 + t_1 * 3U;

#line 1253
            uint _S17 = corner_at_0(corner_1, &kernelContext_4);

#line 1253
            uint _S18 = corner_at_0(corner_1 + 1U, &kernelContext_4);

#line 1253
            uint _S19 = corner_at_0(corner_1 + 2U, &kernelContext_4);

#line 1253
            _slang_mesh.set_index(t_1*3+0,(uint3(_S17, _S18, _S19))[0]);
            _slang_mesh.set_index(t_1*3+1,(uint3(_S17, _S18, _S19))[1]);
            _slang_mesh.set_index(t_1*3+2,(uint3(_S17, _S18, _S19))[2]);
            ;

#line 1253
            t_1 = t_1 + 64U;

#line 1253
        }

#line 1253
        break;
    }

#line 1254
    return;
}


#line 1121
uint cluster_is_selected_0(const ClusterSelect_0 thread* select_1, uint instance_index_0, KernelContext_0 thread* kernelContext_5)
{
    uint base_1 = instance_index_0 * kernelContext_5->draw_0->group_stride_0;

#line 1123
    uint _S20 = select_1->flags_1;

#line 1123
    bool producer_expanded_0;


    if(((select_1->flags_1) & 1U) != 0U)
    {

#line 1126
        producer_expanded_0 = kernelContext_5->group_state_0[base_1 + select_1->producer_group_0] != 0U;

#line 1126
    }
    else
    {

#line 1126
        producer_expanded_0 = false;

#line 1126
    }

#line 1126
    bool container_expanded_0;

    if((_S20 & 2U) == 0U)
    {

#line 1128
        container_expanded_0 = true;

#line 1128
    }
    else
    {

#line 1128
        container_expanded_0 = kernelContext_5->group_state_0[base_1 + select_1->container_group_0] != 0U;

#line 1128
    }

    if(!producer_expanded_0)
    {

#line 1130
        producer_expanded_0 = container_expanded_0;

#line 1130
    }
    else
    {

#line 1130
        producer_expanded_0 = false;

#line 1130
    }

#line 1130
    uint _S21;

#line 1130
    if(producer_expanded_0)
    {

#line 1130
        _S21 = 1U;

#line 1130
    }
    else
    {

#line 1130
        _S21 = 0U;

#line 1130
    }

#line 1130
    return _S21;
}


#line 990
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S22 = (((basis_0) * (transpose(basis_0))));

#line 992
    float bound_0 = 0.0f;

#line 992
    uint row_0 = 0U;

    for(;;)
    {

#line 994
        if(row_0 < 3U)
        {
        }
        else
        {

#line 994
            break;
        }
        float _S23 = max(bound_0, abs(_S22[row_0][int(0)]) + abs(_S22[row_0][int(1)]) + abs(_S22[row_0][int(2)]));

#line 994
        uint row_1 = row_0 + 1U;

#line 994
        bound_0 = _S23;

#line 994
        row_0 = row_1;

#line 994
    }



    return sqrt(bound_0);
}


#line 1064
uint cluster_survives_0(const Meshlet_0 thread* cluster_2, matrix<float,int(4),int(4)>  transform_2, KernelContext_0 thread* kernelContext_6)
{
    matrix<float,int(3),int(3)>  _S24 = matrix<float,int(3),int(3)> (transform_2[int(0)].xyz, transform_2[int(1)].xyz, transform_2[int(2)].xyz);
    float3 center_1 = (((float4(cluster_2->center_x_0, cluster_2->center_y_0, cluster_2->center_z_0, 1.0f)) * (transform_2))).xyz;
    float radius_3 = cluster_2->radius_0 * max_stretch_0(_S24);

#line 1068
    uint plane_0 = 0U;

    for(;;)
    {

#line 1070
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 1070
            break;
        }

        float3 _S25 = kernelContext_6->cull_0->planes_0[plane_0].xyz;

#line 1073
        if((dot(_S25, center_1) + kernelContext_6->cull_0->planes_0[plane_0].w) < (- radius_3 * length(_S25)))
        {
            return 1U;
        }

#line 1070
        plane_0 = plane_0 + 1U;

#line 1070
    }

#line 1087
    float3 axis_0 = (((float3(cluster_2->cone_axis_x_0, cluster_2->cone_axis_y_0, cluster_2->cone_axis_z_0)) * (_S24)));

    float axis_length_0 = length(axis_0);

#line 1089
    float3 axis_1;
    if(axis_length_0 > 0.0f)
    {

#line 1090
        axis_1 = axis_0 / float3(axis_length_0) ;

#line 1090
    }
    else
    {

#line 1090
        axis_1 = float3(0.0f, 0.0f, 0.0f);

#line 1090
    }
    float3 to_center_0 = center_1 - kernelContext_6->frame_0->camera_position_0.xyz;
    float sine_0 = sqrt(max(0.0f, 1.0f - cluster_2->cone_cutoff_0 * cluster_2->cone_cutoff_0));

#line 1092
    bool _S26;

    if((cluster_2->cone_cutoff_0) > 0.0f)
    {

#line 1094
        _S26 = (dot(axis_1, to_center_0)) > (sine_0 * length(to_center_0) + radius_3);

#line 1094
    }
    else
    {

#line 1094
        _S26 = false;

#line 1094
    }

#line 1093
    if(_S26)
    {

        return 2U;
    }

    return 0U;
}


#line 1278
[[object]] void taskMain(uint3 group_3 [[threadgroup_position_in_grid]], ClusterPayload_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp, ClusterDrawConstants_0 constant* draw_2 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_2 [[buffer(10)]], Meshlet_0 device* clusters_2 [[buffer(7)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], ClusterSelect_0 device* cluster_select_2 [[buffer(13)]], uint device* tables_2 [[buffer(19)]], uint device* cluster_vertices_2 [[buffer(8)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], uint device* cluster_corners_2 [[buffer(9)]], uint device* group_state_2 [[buffer(15)]], CullParams_0 constant* cull_2 [[buffer(11)]], atomic<uint> device* cull_stats_2 [[buffer(12)]], uint device* cluster_selection_2 [[buffer(14)]])
{

#line 1278
    thread KernelContext_0 kernelContext_7;

#line 1278
    (&kernelContext_7)->draw_0 = draw_2;

#line 1278
    (&kernelContext_7)->draw_args_0 = draw_args_2;

#line 1278
    (&kernelContext_7)->clusters_0 = clusters_2;

#line 1278
    (&kernelContext_7)->visible_instances_0 = visible_instances_2;

#line 1278
    (&kernelContext_7)->instances_0 = instances_2;

#line 1278
    (&kernelContext_7)->meshes_0 = meshes_2;

#line 1278
    (&kernelContext_7)->frame_0 = frame_2;

#line 1278
    (&kernelContext_7)->cluster_select_0 = cluster_select_2;

#line 1278
    (&kernelContext_7)->tables_0 = tables_2;

#line 1278
    (&kernelContext_7)->cluster_vertices_0 = cluster_vertices_2;

#line 1278
    (&kernelContext_7)->vertices_0 = vertices_2;

#line 1278
    (&kernelContext_7)->cluster_corners_0 = cluster_corners_2;

#line 1278
    (&kernelContext_7)->group_state_0 = group_state_2;

#line 1278
    (&kernelContext_7)->cull_0 = cull_2;

#line 1278
    (&kernelContext_7)->cull_stats_0 = cull_stats_2;

#line 1278
    (&kernelContext_7)->cluster_selection_0 = cluster_selection_2;

#line 1278
    uint _S27 = group_is_live_0(group_3, &kernelContext_7);


    uint _S28 = group_3.x;

#line 1281
    uint _S29 = _S28 * _S27;

#line 1281
    Meshlet_0 cluster_3 = (&kernelContext_7)->clusters_0[(&kernelContext_7)->draw_0->cluster_base_0 + _S29];
    uint _S30 = group_3.y;

#line 1282
    uint instance_index_1 = (&kernelContext_7)->visible_instances_0[(&kernelContext_7)->draw_0->base_0 + _S30 * _S27] * _S27;
    GpuInstance_natural_0 instance_2 = (&kernelContext_7)->instances_0[instance_index_1];

    uint index_0 = (&kernelContext_7)->draw_0->cluster_base_0 + _S29;

#line 1285
    thread ClusterSelect_0 _S31 = (&kernelContext_7)->cluster_select_0[index_0];

#line 1285
    uint _S32 = cluster_is_selected_0(&_S31, instance_index_1, &kernelContext_7);

#line 1285
    matrix<float,int(4),int(4)>  _S33 = matrix<float,int(4),int(4)> (instance_2.transform_0.data_0[int(0)][int(0)], instance_2.transform_0.data_0[int(1)][int(0)], instance_2.transform_0.data_0[int(2)][int(0)], instance_2.transform_0.data_0[int(3)][int(0)], instance_2.transform_0.data_0[int(0)][int(1)], instance_2.transform_0.data_0[int(1)][int(1)], instance_2.transform_0.data_0[int(2)][int(1)], instance_2.transform_0.data_0[int(3)][int(1)], instance_2.transform_0.data_0[int(0)][int(2)], instance_2.transform_0.data_0[int(1)][int(2)], instance_2.transform_0.data_0[int(2)][int(2)], instance_2.transform_0.data_0[int(3)][int(2)], instance_2.transform_0.data_0[int(0)][int(3)], instance_2.transform_0.data_0[int(1)][int(3)], instance_2.transform_0.data_0[int(2)][int(3)], instance_2.transform_0.data_0[int(3)][int(3)]);

#line 1285
    thread Meshlet_0 _S34 = cluster_3;

#line 1285
    uint _S35 = cluster_survives_0(&_S34, _S33, &kernelContext_7);


    uint _S36 = _S27 * _S32;

#line 1288
    bool _S37 = _S35 == 0U;

#line 1288
    uint word_0;

#line 1288
    if(_S37)
    {

#line 1288
        word_0 = 1U;

#line 1288
    }
    else
    {

#line 1288
        word_0 = 0U;

#line 1288
    }

#line 1288
    uint keep_0 = _S36 * word_0;

#line 1301
    if(_S37)
    {

#line 1301
        word_0 = 1U;

#line 1301
    }
    else
    {

#line 1302
        if(_S35 == 1U)
        {

#line 1302
            word_0 = 3U;

#line 1302
        }
        else
        {

#line 1302
            word_0 = 4U;

#line 1302
        }

#line 1301
    }


    if(_S36 == 1U)
    {
        uint _S38 = atomic_fetch_add_explicit((&kernelContext_7)->cull_stats_0+word_0, 1U, memory_order_relaxed);

#line 1304
    }

#line 1304
    bool _S39;

#line 1319
    if(_S30 == 0U)
    {

#line 1319
        _S39 = _S27 == 1U;

#line 1319
    }
    else
    {

#line 1319
        _S39 = false;

#line 1319
    }

#line 1319
    if(_S39)
    {
        *((&kernelContext_7)->cluster_selection_0+index_0) = _S32;

#line 1319
    }

#line 1324
    thread ClusterPayload_0 payload_0;
    (&payload_0)->cluster_0 = _S28;
    (&payload_0)->instance_0 = _S30;
    *_slang_mesh_payload = *(&payload_0); _slang_mgp.set_threadgroups_per_grid(uint3((keep_0), (1U), (1U))); return;;
    return;
}


#line 1339
[[mesh]] void amplifiedMeshMain(uint3 lane_2 [[thread_position_in_threadgroup]], const ClusterPayload_0 object_data* amplification_0 [[payload]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_3 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_3 [[buffer(10)]], Meshlet_0 device* clusters_3 [[buffer(7)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], ClusterSelect_0 device* cluster_select_3 [[buffer(13)]], uint device* tables_3 [[buffer(19)]], uint device* cluster_vertices_3 [[buffer(8)]], MeshVertex_natural_0 device* vertices_3 [[buffer(1)]], uint device* cluster_corners_3 [[buffer(9)]], uint device* group_state_3 [[buffer(15)]], CullParams_0 constant* cull_3 [[buffer(11)]], atomic<uint> device* cull_stats_3 [[buffer(12)]], uint device* cluster_selection_3 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_8;

#line 1341
    (&kernelContext_8)->draw_0 = draw_3;

#line 1341
    (&kernelContext_8)->draw_args_0 = draw_args_3;

#line 1341
    (&kernelContext_8)->clusters_0 = clusters_3;

#line 1341
    (&kernelContext_8)->visible_instances_0 = visible_instances_3;

#line 1341
    (&kernelContext_8)->instances_0 = instances_3;

#line 1341
    (&kernelContext_8)->meshes_0 = meshes_3;

#line 1341
    (&kernelContext_8)->frame_0 = frame_3;

#line 1341
    (&kernelContext_8)->cluster_select_0 = cluster_select_3;

#line 1341
    (&kernelContext_8)->tables_0 = tables_3;

#line 1341
    (&kernelContext_8)->cluster_vertices_0 = cluster_vertices_3;

#line 1341
    (&kernelContext_8)->vertices_0 = vertices_3;

#line 1341
    (&kernelContext_8)->cluster_corners_0 = cluster_corners_3;

#line 1341
    (&kernelContext_8)->group_state_0 = group_state_3;

#line 1341
    (&kernelContext_8)->cull_0 = cull_3;

#line 1341
    (&kernelContext_8)->cull_stats_0 = cull_stats_3;

#line 1341
    (&kernelContext_8)->cluster_selection_0 = cluster_selection_3;

#line 1341
    uint lane_3 = lane_2.x;

#line 1347
    uint _S40 = draw_3->cluster_base_0 + amplification_0->cluster_0;

#line 1345
    uint _S41 = amplification_0->instance_0;

#line 1345
    for(;;)
    {

#line 1345
        Meshlet_0 cluster_4 = (&kernelContext_8)->clusters_0[_S40];

#line 1345
        _slang_mesh.set_primitive_count((cluster_4.triangle_count_0));

#line 1345
        GpuInstance_natural_0 instance_3 = (&kernelContext_8)->instances_0[(&kernelContext_8)->visible_instances_0[(&kernelContext_8)->draw_0->base_0 + _S41]];

#line 1345
        GpuMesh_0 _S42 = (&kernelContext_8)->meshes_0[instance_3.mesh_0];

#line 1345
        float4 _S43 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 1345
        float4 overlay_1;

#line 1345
        if(((&kernelContext_8)->frame_0->ambient_0.w) >= 2.5f)
        {

#line 1345
            float3 _S44 = cluster_heat_0(_S40, matrix<float,int(4),int(4)> (instance_3.transform_0.data_0[int(0)][int(0)], instance_3.transform_0.data_0[int(1)][int(0)], instance_3.transform_0.data_0[int(2)][int(0)], instance_3.transform_0.data_0[int(3)][int(0)], instance_3.transform_0.data_0[int(0)][int(1)], instance_3.transform_0.data_0[int(1)][int(1)], instance_3.transform_0.data_0[int(2)][int(1)], instance_3.transform_0.data_0[int(3)][int(1)], instance_3.transform_0.data_0[int(0)][int(2)], instance_3.transform_0.data_0[int(1)][int(2)], instance_3.transform_0.data_0[int(2)][int(2)], instance_3.transform_0.data_0[int(3)][int(2)], instance_3.transform_0.data_0[int(0)][int(3)], instance_3.transform_0.data_0[int(1)][int(3)], instance_3.transform_0.data_0[int(2)][int(3)], instance_3.transform_0.data_0[int(3)][int(3)]), &kernelContext_8);

#line 1345
            overlay_1 = float4(_S44, 1.0f);

#line 1345
        }
        else
        {

#line 1345
            if(((&kernelContext_8)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1345
                overlay_1 = float4(lod_tint_0(((&kernelContext_8)->cluster_select_0[_S40].flags_1) >> 2U), 1.0f);

#line 1345
            }
            else
            {

#line 1345
                overlay_1 = _S43;

#line 1345
            }

#line 1345
        }

#line 1345
        ClusterSelect_0 _S45 = (&kernelContext_8)->cluster_select_0[_S40];

#line 1345
        uint v_1 = lane_3;

#line 1345
        for(;;)
        {

#line 1345
            if(v_1 < (cluster_4.vertex_count_0))
            {
            }
            else
            {

#line 1345
                break;
            }

#line 1345
            MeshVertex_natural_0 vertex_1 = (&kernelContext_8)->vertices_0[(&kernelContext_8)->cluster_vertices_0[cluster_4.vertex_offset_1 + v_1] + _S42.base_vertex_0 + _S45.vertex_base_0];

#line 1345
            matrix<float,int(4),int(4)>  _S46 = matrix<float,int(4),int(4)> (instance_3.transform_0.data_0[int(0)][int(0)], instance_3.transform_0.data_0[int(1)][int(0)], instance_3.transform_0.data_0[int(2)][int(0)], instance_3.transform_0.data_0[int(3)][int(0)], instance_3.transform_0.data_0[int(0)][int(1)], instance_3.transform_0.data_0[int(1)][int(1)], instance_3.transform_0.data_0[int(2)][int(1)], instance_3.transform_0.data_0[int(3)][int(1)], instance_3.transform_0.data_0[int(0)][int(2)], instance_3.transform_0.data_0[int(1)][int(2)], instance_3.transform_0.data_0[int(2)][int(2)], instance_3.transform_0.data_0[int(3)][int(2)], instance_3.transform_0.data_0[int(0)][int(3)], instance_3.transform_0.data_0[int(1)][int(3)], instance_3.transform_0.data_0[int(2)][int(3)], instance_3.transform_0.data_0[int(3)][int(3)]);

#line 1345
            float4 world_1 = (((float4((float4(vertex_1.position_0) ).xyz, 1.0f)) * (_S46)));

#line 1345
            thread VertexOutput_0 output_1;

#line 1345
            (&output_1)->position_1 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 1345
            (&output_1)->world_position_0 = world_1.xyz;

#line 1345
            (&output_1)->world_normal_0 = ((((float4(vertex_1.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S46[int(0)].xyz, _S46[int(1)].xyz, _S46[int(2)].xyz))));

#line 1345
            float4 _S47;

#line 1345
            if(((&kernelContext_8)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1345
                _S47 = overlay_1;

#line 1345
            }
            else
            {

#line 1345
                _S47 = float4(vertex_1.color_0) ;

#line 1345
            }

#line 1345
            (&output_1)->color_1 = _S47;

#line 1345
            (&output_1)->material_1 = instance_3.material_0;

#line 1345
            (&output_1)->uv_1 = (float4(vertex_1.uv_0) ).xy;

#line 1345
            _slang_mesh.set_vertex(v_1,output_1);

#line 1345
            v_1 = v_1 + 64U;

#line 1345
        }

#line 1345
        uint t_2 = lane_3;

#line 1345
        for(;;)
        {

#line 1345
            if(t_2 < (cluster_4.triangle_count_0))
            {
            }
            else
            {

#line 1345
                break;
            }

#line 1345
            uint corner_2 = cluster_4.triangle_offset_0 + t_2 * 3U;

#line 1345
            uint _S48 = corner_at_0(corner_2, &kernelContext_8);

#line 1345
            uint _S49 = corner_at_0(corner_2 + 1U, &kernelContext_8);

#line 1345
            uint _S50 = corner_at_0(corner_2 + 2U, &kernelContext_8);

#line 1345
            _slang_mesh.set_index(t_2*3+0,(uint3(_S48, _S49, _S50))[0]);
            _slang_mesh.set_index(t_2*3+1,(uint3(_S48, _S49, _S50))[1]);
            _slang_mesh.set_index(t_2*3+2,(uint3(_S48, _S49, _S50))[2]);
            ;

#line 1345
            t_2 = t_2 + 64U;

#line 1345
        }

#line 1345
        break;
    }

#line 1352
    return;
}

