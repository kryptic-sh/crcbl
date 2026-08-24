#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 652 "shaders/mesh_cluster.slang"
struct ClusterPayload_0
{
    uint cluster_0;
    uint instance_0;
};


#line 669
struct ClusterDrawConstants_0
{
    uint base_0;
    uint cluster_base_0;
    uint cluster_count_0;
    uint bucket_0;
    uint group_stride_0;
    uint level_groups_at_0;
};


#line 591
struct DrawIndexedArgs_0
{
    uint index_count_0;
    uint instance_count_0;
    uint first_index_0;
    int vertex_offset_0;
    uint first_instance_0;
};


#line 288
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


#line 1274
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1274
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
    uint base_vertex_0;
    uint pad0_0;
    uint pad1_0;
    uint pad2_0;
};


#line 254
struct GpuMesh_0
{
    uint base_vertex_1;
    uint base_index_0;
    uint index_count_1;
    float min_x_0;
    float min_y_0;
    float min_z_0;
    float max_x_0;
    float max_y_0;
    float max_z_0;
};


#line 1275
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1275
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


#line 567
struct ClusterSelect_0
{
    uint flags_1;
    uint vertex_base_0;
    uint producer_group_0;
    uint container_group_0;
};


#line 996
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 611
struct CullParams_0
{
    array<float4, int(6)> planes_0;
    uint instance_count_1;
    uint capacity_0;
};


#line 1437
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


#line 1026
uint group_is_live_0(uint3 group_0, KernelContext_0 thread* kernelContext_0)
{

    uint _S1 = group_0.y;
    uint _S2 = group_0.x;

#line 1029
    return min(1U, max(kernelContext_0->draw_args_0[kernelContext_0->draw_0->bucket_0].instance_count_0, _S1) - _S1) * min(1U, max(kernelContext_0->draw_0->cluster_count_0, _S2) - _S2);
}


#line 714
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_1;
    float center_y_1;
    float center_z_1;
    float radius_1;
};


#line 953
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


#line 512
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S3 = (((basis_0) * (transpose(basis_0))));

#line 514
    float bound_0 = 0.0f;

#line 514
    uint row_0 = 0U;

    for(;;)
    {

#line 516
        if(row_0 < 3U)
        {
        }
        else
        {

#line 516
            break;
        }
        float _S4 = max(bound_0, abs(_S3[row_0][int(0)]) + abs(_S3[row_0][int(1)]) + abs(_S3[row_0][int(2)]));

#line 516
        uint row_1 = row_0 + 1U;

#line 516
        bound_0 = _S4;

#line 516
        row_0 = row_1;

#line 516
    }



    return sqrt(bound_0);
}


#line 471
float projected_error_0(float error_1, float3 center_0, float radius_2, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S5 = delta_0.x;

#line 474
    float _S6 = delta_0.y;

#line 474
    float _S7 = delta_0.z;
    float distance_0 = sqrt(_S5 * _S5 + _S6 * _S6 + _S7 * _S7) - radius_2;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 430
float3 heat_tint_0(float projected_0, float expand_0, float hold_0)
{
    float _S8 = max(expand_0, 9.99999997475242708e-07f);
    float t_0 = projected_0 / _S8;
    if(t_0 >= 1.0f)
    {
        return float3(1.0f, 1.0f, 1.0f);
    }



    float band_0 = clamp(hold_0 / _S8, 0.0f, 1.0f);
    if(t_0 >= band_0)
    {
        return mix(float3(0.85000002384185791f, 0.44999998807907104f, 0.10000000149011612f), float3(0.97000002861022949f, 0.85000002384185791f, 0.20000000298023224f), float3(saturate((t_0 - band_0) / max(1.0f - band_0, 9.99999997475242708e-07f))) );
    }

    return mix(float3(0.07999999821186066f, 0.10000000149011612f, 0.34999999403953552f), float3(0.10000000149011612f, 0.55000001192092896f, 0.60000002384185791f), float3(saturate(t_0 / max(band_0, 9.99999997475242708e-07f))) );
}


#line 994
float3 cluster_heat_0(uint cluster_index_0, matrix<float,int(4),int(4)>  transform_1, KernelContext_0 thread* kernelContext_2)
{
    ClusterSelect_0 select_0 = kernelContext_2->cluster_select_0[cluster_index_0];

#line 996
    float projected_1;

    if(((select_0.flags_1) & 1U) != 0U)
    {

#line 998
        LevelGroup_0 _S9 = level_group_at_0(select_0.producer_group_0, kernelContext_2);


        float stretch_0 = max_stretch_0(matrix<float,int(3),int(3)> (transform_1[int(0)].xyz, transform_1[int(1)].xyz, transform_1[int(2)].xyz));

#line 1001
        projected_1 = projected_error_0(_S9.error_0 * stretch_0, (((float4(_S9.center_x_1, _S9.center_y_1, _S9.center_z_1, 1.0f)) * (transform_1))).xyz, _S9.radius_1 * stretch_0, kernelContext_2->frame_0->camera_position_0.xyz, kernelContext_2->frame_0->lod_params_0.x);

#line 998
    }
    else
    {

#line 998
        projected_1 = 0.0f;

#line 998
    }

#line 1007
    return heat_tint_0(projected_1, kernelContext_2->frame_0->lod_params_0.y, kernelContext_2->frame_0->lod_params_0.z);
}


#line 536
float3 lod_tint_0(uint level_1)
{
    switch(level_1 % 8U)
    {
    case 0U:
        {

#line 540
            return float3(0.89999997615814209f, 0.25f, 0.25f);
        }
    case 1U:
        {

#line 541
            return float3(0.94999998807907104f, 0.60000002384185791f, 0.20000000298023224f);
        }
    case 2U:
        {

#line 542
            return float3(0.89999997615814209f, 0.89999997615814209f, 0.25f);
        }
    case 3U:
        {

#line 543
            return float3(0.30000001192092896f, 0.85000002384185791f, 0.34999999403953552f);
        }
    case 4U:
        {

#line 544
            return float3(0.25f, 0.80000001192092896f, 0.85000002384185791f);
        }
    case 5U:
        {

#line 545
            return float3(0.30000001192092896f, 0.44999998807907104f, 0.94999998807907104f);
        }
    case 6U:
        {

#line 546
            return float3(0.64999997615814209f, 0.34999999403953552f, 0.89999997615814209f);
        }
    default:
        {

#line 547
            return float3(0.94999998807907104f, 0.44999998807907104f, 0.80000001192092896f);
        }
    }

#line 547
}


#line 1226
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_1)
{
    return matrix<float,int(3),int(3)> (cross(basis_1[int(1)], basis_1[int(2)]), cross(basis_1[int(2)], basis_1[int(0)]), cross(basis_1[int(0)], basis_1[int(1)]));
}


#line 939
uint corner_at_0(uint corner_0, KernelContext_0 thread* kernelContext_3)
{

    return (kernelContext_3->cluster_corners_0[corner_0 >> 2U] >> ((corner_0 & 3U) * 8U)) & 255U;
}


#line 925
struct VertexOutput_0
{
    float4 position_1 [[position]];
    float3 world_position_0 [[user(POSITION0)]];
    float3 world_normal_0 [[user(NORMAL0)]];
    float4 color_1 [[user(COLOR0)]];
    [[flat]] uint material_1 [[user(TEXCOORD0)]];
    float2 uv_1 [[user(TEXCOORD1)]];
};


#line 1362
[[mesh]] void meshMain(uint3 lane_0 [[thread_position_in_threadgroup]], uint3 group_2 [[threadgroup_position_in_grid]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_1 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_1 [[buffer(10)]], Meshlet_0 device* clusters_1 [[buffer(7)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], ClusterSelect_0 device* cluster_select_1 [[buffer(13)]], uint device* tables_1 [[buffer(19)]], uint device* cluster_vertices_1 [[buffer(8)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], uint device* cluster_corners_1 [[buffer(9)]], uint device* group_state_1 [[buffer(15)]], CullParams_0 constant* cull_1 [[buffer(11)]], atomic<uint> device* cull_stats_1 [[buffer(12)]], uint device* cluster_selection_1 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_4;

#line 1364
    (&kernelContext_4)->draw_0 = draw_1;

#line 1364
    (&kernelContext_4)->draw_args_0 = draw_args_1;

#line 1364
    (&kernelContext_4)->clusters_0 = clusters_1;

#line 1364
    (&kernelContext_4)->visible_instances_0 = visible_instances_1;

#line 1364
    (&kernelContext_4)->instances_0 = instances_1;

#line 1364
    (&kernelContext_4)->meshes_0 = meshes_1;

#line 1364
    (&kernelContext_4)->frame_0 = frame_1;

#line 1364
    (&kernelContext_4)->cluster_select_0 = cluster_select_1;

#line 1364
    (&kernelContext_4)->tables_0 = tables_1;

#line 1364
    (&kernelContext_4)->cluster_vertices_0 = cluster_vertices_1;

#line 1364
    (&kernelContext_4)->vertices_0 = vertices_1;

#line 1364
    (&kernelContext_4)->cluster_corners_0 = cluster_corners_1;

#line 1364
    (&kernelContext_4)->group_state_0 = group_state_1;

#line 1364
    (&kernelContext_4)->cull_0 = cull_1;

#line 1364
    (&kernelContext_4)->cull_stats_0 = cull_stats_1;

#line 1364
    (&kernelContext_4)->cluster_selection_0 = cluster_selection_1;

#line 1364
    uint lane_1 = lane_0.x;

#line 1364
    uint _S10 = group_is_live_0(group_2, &kernelContext_4);

#line 1369
    uint _S11 = (&kernelContext_4)->draw_0->cluster_base_0 + group_2.x * _S10;

#line 1369
    uint _S12 = group_2.y;

#line 1369
    for(;;)
    {

#line 1369
        Meshlet_0 cluster_1 = (&kernelContext_4)->clusters_0[_S11];

#line 1369
        _slang_mesh.set_primitive_count((cluster_1.triangle_count_0 * _S10));

#line 1369
        if(_S10 == 0U)
        {

#line 1369
            break;
        }

#line 1369
        GpuInstance_natural_0 instance_1 = (&kernelContext_4)->instances_0[(&kernelContext_4)->visible_instances_0[(&kernelContext_4)->draw_0->base_0 + _S12]];

#line 1369
        GpuMesh_0 mesh_1 = (&kernelContext_4)->meshes_0[instance_1.mesh_0];

#line 1369
        float4 _S13 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 1369
        float4 overlay_0;

#line 1369
        if(((&kernelContext_4)->frame_0->ambient_0.w) >= 2.5f)
        {

#line 1369
            float3 _S14 = cluster_heat_0(_S11, matrix<float,int(4),int(4)> (instance_1.transform_0.data_0[int(0)][int(0)], instance_1.transform_0.data_0[int(1)][int(0)], instance_1.transform_0.data_0[int(2)][int(0)], instance_1.transform_0.data_0[int(3)][int(0)], instance_1.transform_0.data_0[int(0)][int(1)], instance_1.transform_0.data_0[int(1)][int(1)], instance_1.transform_0.data_0[int(2)][int(1)], instance_1.transform_0.data_0[int(3)][int(1)], instance_1.transform_0.data_0[int(0)][int(2)], instance_1.transform_0.data_0[int(1)][int(2)], instance_1.transform_0.data_0[int(2)][int(2)], instance_1.transform_0.data_0[int(3)][int(2)], instance_1.transform_0.data_0[int(0)][int(3)], instance_1.transform_0.data_0[int(1)][int(3)], instance_1.transform_0.data_0[int(2)][int(3)], instance_1.transform_0.data_0[int(3)][int(3)]), &kernelContext_4);

#line 1369
            overlay_0 = float4(_S14, 1.0f);

#line 1369
        }
        else
        {

#line 1369
            if(((&kernelContext_4)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1369
                overlay_0 = float4(lod_tint_0(((&kernelContext_4)->cluster_select_0[_S11].flags_1) >> 2U), 1.0f);

#line 1369
            }
            else
            {

#line 1369
                overlay_0 = _S13;

#line 1369
            }

#line 1369
        }

#line 1369
        ClusterSelect_0 _S15 = (&kernelContext_4)->cluster_select_0[_S11];

#line 1369
        uint t_1;

#line 1369
        if(((instance_1.flags_0) & 2U) != 0U)
        {

#line 1369
            t_1 = instance_1.base_vertex_0;

#line 1369
        }
        else
        {

#line 1369
            t_1 = mesh_1.base_vertex_1;

#line 1369
        }

#line 1369
        matrix<float,int(4),int(4)>  _S16 = matrix<float,int(4),int(4)> (instance_1.transform_0.data_0[int(0)][int(0)], instance_1.transform_0.data_0[int(1)][int(0)], instance_1.transform_0.data_0[int(2)][int(0)], instance_1.transform_0.data_0[int(3)][int(0)], instance_1.transform_0.data_0[int(0)][int(1)], instance_1.transform_0.data_0[int(1)][int(1)], instance_1.transform_0.data_0[int(2)][int(1)], instance_1.transform_0.data_0[int(3)][int(1)], instance_1.transform_0.data_0[int(0)][int(2)], instance_1.transform_0.data_0[int(1)][int(2)], instance_1.transform_0.data_0[int(2)][int(2)], instance_1.transform_0.data_0[int(3)][int(2)], instance_1.transform_0.data_0[int(0)][int(3)], instance_1.transform_0.data_0[int(1)][int(3)], instance_1.transform_0.data_0[int(2)][int(3)], instance_1.transform_0.data_0[int(3)][int(3)]);

#line 1369
        matrix<float,int(3),int(3)>  _S17 = normal_basis_0(matrix<float,int(3),int(3)> (_S16[int(0)].xyz, _S16[int(1)].xyz, _S16[int(2)].xyz));

#line 1369
        uint v_0 = lane_1;

#line 1369
        for(;;)
        {

#line 1369
            if(v_0 < (cluster_1.vertex_count_0))
            {
            }
            else
            {

#line 1369
                break;
            }

#line 1369
            MeshVertex_natural_0 vertex_0 = (&kernelContext_4)->vertices_0[(&kernelContext_4)->cluster_vertices_0[cluster_1.vertex_offset_1 + v_0] + t_1 + _S15.vertex_base_0];

#line 1369
            float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S16)));

#line 1369
            thread VertexOutput_0 output_0;

#line 1369
            (&output_0)->position_1 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_4)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_4)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 1369
            (&output_0)->world_position_0 = world_0.xyz;

#line 1369
            (&output_0)->world_normal_0 = ((((float4(vertex_0.normal_0) ).xyz) * (_S17)));

#line 1369
            float4 _S18;

#line 1369
            if(((&kernelContext_4)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1369
                _S18 = overlay_0;

#line 1369
            }
            else
            {

#line 1369
                _S18 = float4(vertex_0.color_0) ;

#line 1369
            }

#line 1369
            (&output_0)->color_1 = _S18;

#line 1369
            (&output_0)->material_1 = instance_1.material_0;

#line 1369
            (&output_0)->uv_1 = (float4(vertex_0.uv_0) ).xy;

#line 1369
            _slang_mesh.set_vertex(v_0,output_0);

#line 1369
            v_0 = v_0 + 64U;

#line 1369
        }

#line 1369
        t_1 = lane_1;

#line 1369
        for(;;)
        {

#line 1369
            if(t_1 < (cluster_1.triangle_count_0))
            {
            }
            else
            {

#line 1369
                break;
            }

#line 1369
            uint corner_1 = cluster_1.triangle_offset_0 + t_1 * 3U;

#line 1369
            uint _S19 = corner_at_0(corner_1, &kernelContext_4);

#line 1369
            uint _S20 = corner_at_0(corner_1 + 1U, &kernelContext_4);

#line 1369
            uint _S21 = corner_at_0(corner_1 + 2U, &kernelContext_4);

#line 1369
            _slang_mesh.set_index(t_1*3+0,(uint3(_S19, _S20, _S21))[0]);
            _slang_mesh.set_index(t_1*3+1,(uint3(_S19, _S20, _S21))[1]);
            _slang_mesh.set_index(t_1*3+2,(uint3(_S19, _S20, _S21))[2]);
            ;

#line 1369
            t_1 = t_1 + 64U;

#line 1369
        }

#line 1369
        break;
    }

#line 1370
    return;
}


#line 1200
uint cluster_is_selected_0(const ClusterSelect_0 thread* select_1, uint instance_index_0, KernelContext_0 thread* kernelContext_5)
{
    uint base_1 = instance_index_0 * kernelContext_5->draw_0->group_stride_0;

#line 1202
    uint _S22 = select_1->flags_1;

#line 1202
    bool producer_expanded_0;


    if(((select_1->flags_1) & 1U) != 0U)
    {

#line 1205
        producer_expanded_0 = kernelContext_5->group_state_0[base_1 + select_1->producer_group_0] != 0U;

#line 1205
    }
    else
    {

#line 1205
        producer_expanded_0 = false;

#line 1205
    }

#line 1205
    bool container_expanded_0;

    if((_S22 & 2U) == 0U)
    {

#line 1207
        container_expanded_0 = true;

#line 1207
    }
    else
    {

#line 1207
        container_expanded_0 = kernelContext_5->group_state_0[base_1 + select_1->container_group_0] != 0U;

#line 1207
    }

    if(!producer_expanded_0)
    {

#line 1209
        producer_expanded_0 = container_expanded_0;

#line 1209
    }
    else
    {

#line 1209
        producer_expanded_0 = false;

#line 1209
    }

#line 1209
    uint _S23;

#line 1209
    if(producer_expanded_0)
    {

#line 1209
        _S23 = 1U;

#line 1209
    }
    else
    {

#line 1209
        _S23 = 0U;

#line 1209
    }

#line 1209
    return _S23;
}


#line 1062
bool preserves_angles_0(matrix<float,int(3),int(3)>  basis_2)
{
    matrix<float,int(3),int(3)>  gram_0 = (((basis_2) * (transpose(basis_2))));
    float _S24 = max(gram_0[int(0)][int(0)], max(gram_0[int(1)][int(1)], gram_0[int(2)][int(2)]));
    if(_S24 <= 0.0f)
    {
        return false;
    }
    float slack_0 = 0.00009999999747379f * _S24;

#line 1070
    bool _S25;
    if((abs(gram_0[int(0)][int(1)])) <= slack_0)
    {

#line 1071
        _S25 = (abs(gram_0[int(0)][int(2)])) <= slack_0;

#line 1071
    }
    else
    {

#line 1071
        _S25 = false;

#line 1071
    }

#line 1071
    if(_S25)
    {

#line 1071
        _S25 = (abs(gram_0[int(1)][int(2)])) <= slack_0;

#line 1071
    }
    else
    {

#line 1071
        _S25 = false;

#line 1071
    }
    if(_S25)
    {

#line 1072
        _S25 = (_S24 - gram_0[int(0)][int(0)]) <= slack_0;

#line 1072
    }
    else
    {

#line 1072
        _S25 = false;

#line 1072
    }

#line 1072
    if(_S25)
    {

#line 1072
        _S25 = (_S24 - gram_0[int(1)][int(1)]) <= slack_0;

#line 1072
    }
    else
    {

#line 1072
        _S25 = false;

#line 1072
    }
    if(_S25)
    {

#line 1073
        _S25 = (_S24 - gram_0[int(2)][int(2)]) <= slack_0;

#line 1073
    }
    else
    {

#line 1073
        _S25 = false;

#line 1073
    }

#line 1071
    return _S25;
}


#line 1143
uint cluster_survives_0(const Meshlet_0 thread* cluster_2, matrix<float,int(4),int(4)>  transform_2, KernelContext_0 thread* kernelContext_6)
{
    matrix<float,int(3),int(3)>  _S26 = matrix<float,int(3),int(3)> (transform_2[int(0)].xyz, transform_2[int(1)].xyz, transform_2[int(2)].xyz);
    float3 center_1 = (((float4(cluster_2->center_x_0, cluster_2->center_y_0, cluster_2->center_z_0, 1.0f)) * (transform_2))).xyz;
    float radius_3 = cluster_2->radius_0 * max_stretch_0(_S26);

#line 1147
    uint plane_0 = 0U;

    for(;;)
    {

#line 1149
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 1149
            break;
        }

        float3 _S27 = kernelContext_6->cull_0->planes_0[plane_0].xyz;

#line 1152
        if((dot(_S27, center_1) + kernelContext_6->cull_0->planes_0[plane_0].w) < (- radius_3 * length(_S27)))
        {
            return 1U;
        }

#line 1149
        plane_0 = plane_0 + 1U;

#line 1149
    }

#line 1166
    float3 axis_0 = (((float3(cluster_2->cone_axis_x_0, cluster_2->cone_axis_y_0, cluster_2->cone_axis_z_0)) * (_S26)));

    float axis_length_0 = length(axis_0);

#line 1168
    float3 axis_1;
    if(axis_length_0 > 0.0f)
    {

#line 1169
        axis_1 = axis_0 / float3(axis_length_0) ;

#line 1169
    }
    else
    {

#line 1169
        axis_1 = float3(0.0f, 0.0f, 0.0f);

#line 1169
    }
    float3 to_center_0 = center_1 - kernelContext_6->frame_0->camera_position_0.xyz;

#line 1170
    float _S28 = cluster_2->cone_cutoff_0;
    float sine_0 = sqrt(max(0.0f, 1.0f - cluster_2->cone_cutoff_0 * cluster_2->cone_cutoff_0));

#line 1171
    bool _S29;
    if(preserves_angles_0(_S26))
    {

#line 1172
        _S29 = _S28 > 0.0f;

#line 1172
    }
    else
    {

#line 1172
        _S29 = false;

#line 1172
    }
    if(_S29)
    {

#line 1173
        _S29 = (dot(axis_1, to_center_0)) > (sine_0 * length(to_center_0) + radius_3);

#line 1173
    }
    else
    {

#line 1173
        _S29 = false;

#line 1173
    }

#line 1172
    if(_S29)
    {

        return 2U;
    }

    return 0U;
}


#line 1394
[[object]] void taskMain(uint3 group_3 [[threadgroup_position_in_grid]], ClusterPayload_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp, ClusterDrawConstants_0 constant* draw_2 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_2 [[buffer(10)]], Meshlet_0 device* clusters_2 [[buffer(7)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], ClusterSelect_0 device* cluster_select_2 [[buffer(13)]], uint device* tables_2 [[buffer(19)]], uint device* cluster_vertices_2 [[buffer(8)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], uint device* cluster_corners_2 [[buffer(9)]], uint device* group_state_2 [[buffer(15)]], CullParams_0 constant* cull_2 [[buffer(11)]], atomic<uint> device* cull_stats_2 [[buffer(12)]], uint device* cluster_selection_2 [[buffer(14)]])
{

#line 1394
    thread KernelContext_0 kernelContext_7;

#line 1394
    (&kernelContext_7)->draw_0 = draw_2;

#line 1394
    (&kernelContext_7)->draw_args_0 = draw_args_2;

#line 1394
    (&kernelContext_7)->clusters_0 = clusters_2;

#line 1394
    (&kernelContext_7)->visible_instances_0 = visible_instances_2;

#line 1394
    (&kernelContext_7)->instances_0 = instances_2;

#line 1394
    (&kernelContext_7)->meshes_0 = meshes_2;

#line 1394
    (&kernelContext_7)->frame_0 = frame_2;

#line 1394
    (&kernelContext_7)->cluster_select_0 = cluster_select_2;

#line 1394
    (&kernelContext_7)->tables_0 = tables_2;

#line 1394
    (&kernelContext_7)->cluster_vertices_0 = cluster_vertices_2;

#line 1394
    (&kernelContext_7)->vertices_0 = vertices_2;

#line 1394
    (&kernelContext_7)->cluster_corners_0 = cluster_corners_2;

#line 1394
    (&kernelContext_7)->group_state_0 = group_state_2;

#line 1394
    (&kernelContext_7)->cull_0 = cull_2;

#line 1394
    (&kernelContext_7)->cull_stats_0 = cull_stats_2;

#line 1394
    (&kernelContext_7)->cluster_selection_0 = cluster_selection_2;

#line 1394
    uint _S30 = group_is_live_0(group_3, &kernelContext_7);


    uint _S31 = group_3.x;

#line 1397
    uint _S32 = _S31 * _S30;

#line 1397
    Meshlet_0 cluster_3 = (&kernelContext_7)->clusters_0[(&kernelContext_7)->draw_0->cluster_base_0 + _S32];
    uint _S33 = group_3.y;

#line 1398
    uint instance_index_1 = (&kernelContext_7)->visible_instances_0[(&kernelContext_7)->draw_0->base_0 + _S33 * _S30] * _S30;
    GpuInstance_natural_0 instance_2 = (&kernelContext_7)->instances_0[instance_index_1];

    uint index_0 = (&kernelContext_7)->draw_0->cluster_base_0 + _S32;

#line 1401
    thread ClusterSelect_0 _S34 = (&kernelContext_7)->cluster_select_0[index_0];

#line 1401
    uint _S35 = cluster_is_selected_0(&_S34, instance_index_1, &kernelContext_7);

#line 1401
    matrix<float,int(4),int(4)>  _S36 = matrix<float,int(4),int(4)> (instance_2.transform_0.data_0[int(0)][int(0)], instance_2.transform_0.data_0[int(1)][int(0)], instance_2.transform_0.data_0[int(2)][int(0)], instance_2.transform_0.data_0[int(3)][int(0)], instance_2.transform_0.data_0[int(0)][int(1)], instance_2.transform_0.data_0[int(1)][int(1)], instance_2.transform_0.data_0[int(2)][int(1)], instance_2.transform_0.data_0[int(3)][int(1)], instance_2.transform_0.data_0[int(0)][int(2)], instance_2.transform_0.data_0[int(1)][int(2)], instance_2.transform_0.data_0[int(2)][int(2)], instance_2.transform_0.data_0[int(3)][int(2)], instance_2.transform_0.data_0[int(0)][int(3)], instance_2.transform_0.data_0[int(1)][int(3)], instance_2.transform_0.data_0[int(2)][int(3)], instance_2.transform_0.data_0[int(3)][int(3)]);

#line 1401
    thread Meshlet_0 _S37 = cluster_3;

#line 1401
    uint _S38 = cluster_survives_0(&_S37, _S36, &kernelContext_7);


    uint _S39 = _S30 * _S35;

#line 1404
    bool _S40 = _S38 == 0U;

#line 1404
    uint word_0;

#line 1404
    if(_S40)
    {

#line 1404
        word_0 = 1U;

#line 1404
    }
    else
    {

#line 1404
        word_0 = 0U;

#line 1404
    }

#line 1404
    uint keep_0 = _S39 * word_0;

#line 1417
    if(_S40)
    {

#line 1417
        word_0 = 1U;

#line 1417
    }
    else
    {

#line 1418
        if(_S38 == 1U)
        {

#line 1418
            word_0 = 3U;

#line 1418
        }
        else
        {

#line 1418
            word_0 = 4U;

#line 1418
        }

#line 1417
    }


    if(_S39 == 1U)
    {
        uint _S41 = atomic_fetch_add_explicit((&kernelContext_7)->cull_stats_0+word_0, 1U, memory_order_relaxed);

#line 1420
    }

#line 1420
    bool _S42;

#line 1435
    if(_S33 == 0U)
    {

#line 1435
        _S42 = _S30 == 1U;

#line 1435
    }
    else
    {

#line 1435
        _S42 = false;

#line 1435
    }

#line 1435
    if(_S42)
    {
        *((&kernelContext_7)->cluster_selection_0+index_0) = _S35;

#line 1435
    }

#line 1440
    thread ClusterPayload_0 payload_0;
    (&payload_0)->cluster_0 = _S31;
    (&payload_0)->instance_0 = _S33;
    *_slang_mesh_payload = *(&payload_0); _slang_mgp.set_threadgroups_per_grid(uint3((keep_0), (1U), (1U))); return;;
    return;
}


#line 1455
[[mesh]] void amplifiedMeshMain(uint3 lane_2 [[thread_position_in_threadgroup]], const ClusterPayload_0 object_data* amplification_0 [[payload]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_3 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_3 [[buffer(10)]], Meshlet_0 device* clusters_3 [[buffer(7)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], ClusterSelect_0 device* cluster_select_3 [[buffer(13)]], uint device* tables_3 [[buffer(19)]], uint device* cluster_vertices_3 [[buffer(8)]], MeshVertex_natural_0 device* vertices_3 [[buffer(1)]], uint device* cluster_corners_3 [[buffer(9)]], uint device* group_state_3 [[buffer(15)]], CullParams_0 constant* cull_3 [[buffer(11)]], atomic<uint> device* cull_stats_3 [[buffer(12)]], uint device* cluster_selection_3 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_8;

#line 1457
    (&kernelContext_8)->draw_0 = draw_3;

#line 1457
    (&kernelContext_8)->draw_args_0 = draw_args_3;

#line 1457
    (&kernelContext_8)->clusters_0 = clusters_3;

#line 1457
    (&kernelContext_8)->visible_instances_0 = visible_instances_3;

#line 1457
    (&kernelContext_8)->instances_0 = instances_3;

#line 1457
    (&kernelContext_8)->meshes_0 = meshes_3;

#line 1457
    (&kernelContext_8)->frame_0 = frame_3;

#line 1457
    (&kernelContext_8)->cluster_select_0 = cluster_select_3;

#line 1457
    (&kernelContext_8)->tables_0 = tables_3;

#line 1457
    (&kernelContext_8)->cluster_vertices_0 = cluster_vertices_3;

#line 1457
    (&kernelContext_8)->vertices_0 = vertices_3;

#line 1457
    (&kernelContext_8)->cluster_corners_0 = cluster_corners_3;

#line 1457
    (&kernelContext_8)->group_state_0 = group_state_3;

#line 1457
    (&kernelContext_8)->cull_0 = cull_3;

#line 1457
    (&kernelContext_8)->cull_stats_0 = cull_stats_3;

#line 1457
    (&kernelContext_8)->cluster_selection_0 = cluster_selection_3;

#line 1457
    uint lane_3 = lane_2.x;

#line 1463
    uint _S43 = draw_3->cluster_base_0 + amplification_0->cluster_0;

#line 1461
    uint _S44 = amplification_0->instance_0;

#line 1461
    for(;;)
    {

#line 1461
        Meshlet_0 cluster_4 = (&kernelContext_8)->clusters_0[_S43];

#line 1461
        _slang_mesh.set_primitive_count((cluster_4.triangle_count_0));

#line 1461
        GpuInstance_natural_0 instance_3 = (&kernelContext_8)->instances_0[(&kernelContext_8)->visible_instances_0[(&kernelContext_8)->draw_0->base_0 + _S44]];

#line 1461
        GpuMesh_0 mesh_2 = (&kernelContext_8)->meshes_0[instance_3.mesh_0];

#line 1461
        float4 _S45 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 1461
        float4 overlay_1;

#line 1461
        if(((&kernelContext_8)->frame_0->ambient_0.w) >= 2.5f)
        {

#line 1461
            float3 _S46 = cluster_heat_0(_S43, matrix<float,int(4),int(4)> (instance_3.transform_0.data_0[int(0)][int(0)], instance_3.transform_0.data_0[int(1)][int(0)], instance_3.transform_0.data_0[int(2)][int(0)], instance_3.transform_0.data_0[int(3)][int(0)], instance_3.transform_0.data_0[int(0)][int(1)], instance_3.transform_0.data_0[int(1)][int(1)], instance_3.transform_0.data_0[int(2)][int(1)], instance_3.transform_0.data_0[int(3)][int(1)], instance_3.transform_0.data_0[int(0)][int(2)], instance_3.transform_0.data_0[int(1)][int(2)], instance_3.transform_0.data_0[int(2)][int(2)], instance_3.transform_0.data_0[int(3)][int(2)], instance_3.transform_0.data_0[int(0)][int(3)], instance_3.transform_0.data_0[int(1)][int(3)], instance_3.transform_0.data_0[int(2)][int(3)], instance_3.transform_0.data_0[int(3)][int(3)]), &kernelContext_8);

#line 1461
            overlay_1 = float4(_S46, 1.0f);

#line 1461
        }
        else
        {

#line 1461
            if(((&kernelContext_8)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1461
                overlay_1 = float4(lod_tint_0(((&kernelContext_8)->cluster_select_0[_S43].flags_1) >> 2U), 1.0f);

#line 1461
            }
            else
            {

#line 1461
                overlay_1 = _S45;

#line 1461
            }

#line 1461
        }

#line 1461
        ClusterSelect_0 _S47 = (&kernelContext_8)->cluster_select_0[_S43];

#line 1461
        uint t_2;

#line 1461
        if(((instance_3.flags_0) & 2U) != 0U)
        {

#line 1461
            t_2 = instance_3.base_vertex_0;

#line 1461
        }
        else
        {

#line 1461
            t_2 = mesh_2.base_vertex_1;

#line 1461
        }

#line 1461
        matrix<float,int(4),int(4)>  _S48 = matrix<float,int(4),int(4)> (instance_3.transform_0.data_0[int(0)][int(0)], instance_3.transform_0.data_0[int(1)][int(0)], instance_3.transform_0.data_0[int(2)][int(0)], instance_3.transform_0.data_0[int(3)][int(0)], instance_3.transform_0.data_0[int(0)][int(1)], instance_3.transform_0.data_0[int(1)][int(1)], instance_3.transform_0.data_0[int(2)][int(1)], instance_3.transform_0.data_0[int(3)][int(1)], instance_3.transform_0.data_0[int(0)][int(2)], instance_3.transform_0.data_0[int(1)][int(2)], instance_3.transform_0.data_0[int(2)][int(2)], instance_3.transform_0.data_0[int(3)][int(2)], instance_3.transform_0.data_0[int(0)][int(3)], instance_3.transform_0.data_0[int(1)][int(3)], instance_3.transform_0.data_0[int(2)][int(3)], instance_3.transform_0.data_0[int(3)][int(3)]);

#line 1461
        matrix<float,int(3),int(3)>  _S49 = normal_basis_0(matrix<float,int(3),int(3)> (_S48[int(0)].xyz, _S48[int(1)].xyz, _S48[int(2)].xyz));

#line 1461
        uint v_1 = lane_3;

#line 1461
        for(;;)
        {

#line 1461
            if(v_1 < (cluster_4.vertex_count_0))
            {
            }
            else
            {

#line 1461
                break;
            }

#line 1461
            MeshVertex_natural_0 vertex_1 = (&kernelContext_8)->vertices_0[(&kernelContext_8)->cluster_vertices_0[cluster_4.vertex_offset_1 + v_1] + t_2 + _S47.vertex_base_0];

#line 1461
            float4 world_1 = (((float4((float4(vertex_1.position_0) ).xyz, 1.0f)) * (_S48)));

#line 1461
            thread VertexOutput_0 output_1;

#line 1461
            (&output_1)->position_1 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 1461
            (&output_1)->world_position_0 = world_1.xyz;

#line 1461
            (&output_1)->world_normal_0 = ((((float4(vertex_1.normal_0) ).xyz) * (_S49)));

#line 1461
            float4 _S50;

#line 1461
            if(((&kernelContext_8)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1461
                _S50 = overlay_1;

#line 1461
            }
            else
            {

#line 1461
                _S50 = float4(vertex_1.color_0) ;

#line 1461
            }

#line 1461
            (&output_1)->color_1 = _S50;

#line 1461
            (&output_1)->material_1 = instance_3.material_0;

#line 1461
            (&output_1)->uv_1 = (float4(vertex_1.uv_0) ).xy;

#line 1461
            _slang_mesh.set_vertex(v_1,output_1);

#line 1461
            v_1 = v_1 + 64U;

#line 1461
        }

#line 1461
        t_2 = lane_3;

#line 1461
        for(;;)
        {

#line 1461
            if(t_2 < (cluster_4.triangle_count_0))
            {
            }
            else
            {

#line 1461
                break;
            }

#line 1461
            uint corner_2 = cluster_4.triangle_offset_0 + t_2 * 3U;

#line 1461
            uint _S51 = corner_at_0(corner_2, &kernelContext_8);

#line 1461
            uint _S52 = corner_at_0(corner_2 + 1U, &kernelContext_8);

#line 1461
            uint _S53 = corner_at_0(corner_2 + 2U, &kernelContext_8);

#line 1461
            _slang_mesh.set_index(t_2*3+0,(uint3(_S51, _S52, _S53))[0]);
            _slang_mesh.set_index(t_2*3+1,(uint3(_S51, _S52, _S53))[1]);
            _slang_mesh.set_index(t_2*3+2,(uint3(_S51, _S52, _S53))[2]);
            ;

#line 1461
            t_2 = t_2 + 64U;

#line 1461
        }

#line 1461
        break;
    }

#line 1468
    return;
}

