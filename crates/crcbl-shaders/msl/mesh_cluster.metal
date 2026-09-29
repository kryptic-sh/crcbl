#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 919 "shaders/mesh_cluster.slang"
struct ClusterPayload_0
{
    array<uint, int(32)> pairs_0;
    uint bucket_offset_0;
};


#line 1195
struct ClusterSource_0
{
    uint start_at_0;
    uint cluster_base_0;
    uint cluster_count_0;
    uint bucket_0;
};


#line 941
struct ClusterDrawConstants_0
{
    uint start_at_1;
    uint cluster_base_1;
    uint cluster_count_1;
    uint bucket_1;
    uint group_stride_0;
    uint level_groups_at_0;
    uint cluster_base_at_0;
    uint cluster_count_at_0;
    uint chunk_starts_at_0;
    uint chunk_starts_end_0;
};


#line 820
struct DrawIndexedArgs_0
{
    uint index_count_0;
    uint instance_count_0;
    uint first_index_0;
    int vertex_offset_0;
    uint first_instance_0;
};


#line 516
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


#line 1730
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1730
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
    uint base_vertex_0;
    uint previous_base_vertex_0;
    uint pad1_0;
    uint pad2_0;
};


#line 470
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
    float uv_scale_u_0;
    float uv_scale_v_0;
    float uv_offset_u_0;
    float uv_offset_v_0;
    uint flags_1;
};


#line 1732
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1732
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 254
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(14)> data_3;
};


#line 254
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 ambient_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
    uint4 cluster_grid_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0 light_view_proj_0;
    uint4 probe_counts_0;
    uint4 probe_levels_0;
    array<float4, int(4)> probe_level_origin_0;
    array<float4, int(4)> probe_level_inv_spacing_0;
    array<uint4, int(4)> probe_level_offset_0;
    float4 lod_params_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sky_sh_r_0;
    float4 sky_sh_g_0;
    float4 sky_sh_b_0;
    _MatrixStorage_float4x4_ColMajornatural_1 previous_view_proj_0;
    uint4 vertex_pool_0;
    array<float4, int(16)> shadow_atlas_rect_0;
    uint4 shadow_filter_0;
};


#line 795
struct ClusterSelect_0
{
    uint flags_2;
    uint vertex_base_0;
    uint producer_group_0;
    uint container_group_0;
};


#line 844
struct CullParams_natural_0
{
    array<float4, int(6)> planes_0;
    uint instance_count_1;
    uint capacity_0;
    uint hidden_view_0;
    uint features_0;
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_1;
    _MatrixStorage_float4x4_ColMajornatural_1 previous_view_proj_1;
    uint target_width_0;
    uint target_height_0;
    uint pyramid_levels_0;
    uint occlusion_pad_0;
    float small_feature_pixels_0;
    float small_feature_pad0_0;
    float small_feature_pad1_0;
    float small_feature_pad2_0;
    array<float4, int(24)> face_planes_0;
};


#line 2032
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
    uint device* vertices_0;
    uint device* cluster_corners_0;
    uint device* group_state_0;
    CullParams_natural_0 constant* cull_0;
    atomic<uint> device* cull_stats_0;
    uint device* cluster_selection_0;
    array<uint, int(32)> threadgroup* task_kept_0;
    ClusterPayload_0 threadgroup* task_payload_0;
};


#line 1222
ClusterSource_0 cluster_source_0(uint draw_index_0, KernelContext_0 thread* kernelContext_0)
{



    thread ClusterSource_0 source_0;
    (&source_0)->start_at_0 = kernelContext_0->draw_0->start_at_1;
    (&source_0)->cluster_base_0 = kernelContext_0->draw_0->cluster_base_1;
    (&source_0)->cluster_count_0 = kernelContext_0->draw_0->cluster_count_1;
    (&source_0)->bucket_0 = kernelContext_0->draw_0->bucket_1;
    return source_0;
}


#line 1470
uint group_is_live_0(uint3 group_0, const ClusterSource_0 thread* source_1, KernelContext_0 thread* kernelContext_1)
{

    uint _S1 = group_0.y;
    uint _S2 = group_0.x;

#line 1473
    return min(1U, max(kernelContext_1->draw_args_0[source_1->bucket_0].instance_count_0, _S1) - _S1) * min(1U, max(source_1->cluster_count_0, _S2) - _S2);
}


#line 1010
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_1;
    float center_y_1;
    float center_z_1;
    float radius_1;
};


#line 1397
LevelGroup_0 level_group_at_0(uint group_1, KernelContext_0 thread* kernelContext_2)
{
    uint at_0 = kernelContext_2->draw_0->level_groups_at_0 + group_1 * 6U;
    thread LevelGroup_0 record_0;
    (&record_0)->level_0 = kernelContext_2->tables_0[at_0];
    (&record_0)->error_0 = (as_type<float>((kernelContext_2->tables_0[at_0 + 1U])));
    (&record_0)->center_x_1 = (as_type<float>((kernelContext_2->tables_0[at_0 + 2U])));
    (&record_0)->center_y_1 = (as_type<float>((kernelContext_2->tables_0[at_0 + 3U])));
    (&record_0)->center_z_1 = (as_type<float>((kernelContext_2->tables_0[at_0 + 4U])));
    (&record_0)->radius_1 = (as_type<float>((kernelContext_2->tables_0[at_0 + 5U])));
    return record_0;
}


#line 740
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S3 = (((basis_0) * (transpose(basis_0))));

#line 742
    float bound_0 = 0.0f;

#line 742
    uint row_0 = 0U;

    for(;;)
    {

#line 744
        if(row_0 < 3U)
        {
        }
        else
        {

#line 744
            break;
        }
        float _S4 = max(bound_0, abs(_S3[row_0][int(0)]) + abs(_S3[row_0][int(1)]) + abs(_S3[row_0][int(2)]));

#line 744
        uint row_1 = row_0 + 1U;

#line 744
        bound_0 = _S4;

#line 744
        row_0 = row_1;

#line 744
    }



    return sqrt(bound_0);
}


#line 699
float projected_error_0(float error_1, float3 center_0, float radius_2, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S5 = delta_0.x;

#line 702
    float _S6 = delta_0.y;

#line 702
    float _S7 = delta_0.z;
    float distance_0 = sqrt(_S5 * _S5 + _S6 * _S6 + _S7 * _S7) - radius_2;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 658
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


#line 1438
float3 cluster_heat_0(uint cluster_index_0, matrix<float,int(4),int(4)>  transform_1, KernelContext_0 thread* kernelContext_3)
{
    ClusterSelect_0 select_0 = kernelContext_3->cluster_select_0[cluster_index_0];

#line 1440
    float projected_1;

    if(((select_0.flags_2) & 1U) != 0U)
    {

#line 1442
        LevelGroup_0 _S9 = level_group_at_0(select_0.producer_group_0, kernelContext_3);


        float stretch_0 = max_stretch_0(matrix<float,int(3),int(3)> (transform_1[int(0)].xyz, transform_1[int(1)].xyz, transform_1[int(2)].xyz));

#line 1445
        projected_1 = projected_error_0(_S9.error_0 * stretch_0, (((float4(_S9.center_x_1, _S9.center_y_1, _S9.center_z_1, 1.0f)) * (transform_1))).xyz, _S9.radius_1 * stretch_0, kernelContext_3->frame_0->camera_position_0.xyz, kernelContext_3->frame_0->lod_params_0.x);

#line 1442
    }
    else
    {

#line 1442
        projected_1 = 0.0f;

#line 1442
    }

#line 1451
    return heat_tint_0(projected_1, kernelContext_3->frame_0->lod_params_0.y, kernelContext_3->frame_0->lod_params_0.z);
}


#line 764
float3 lod_tint_0(uint level_1)
{
    switch(level_1 % 8U)
    {
    case 0U:
        {

#line 768
            return float3(0.89999997615814209f, 0.25f, 0.25f);
        }
    case 1U:
        {

#line 769
            return float3(0.94999998807907104f, 0.60000002384185791f, 0.20000000298023224f);
        }
    case 2U:
        {

#line 770
            return float3(0.89999997615814209f, 0.89999997615814209f, 0.25f);
        }
    case 3U:
        {

#line 771
            return float3(0.30000001192092896f, 0.85000002384185791f, 0.34999999403953552f);
        }
    case 4U:
        {

#line 772
            return float3(0.25f, 0.80000001192092896f, 0.85000002384185791f);
        }
    case 5U:
        {

#line 773
            return float3(0.30000001192092896f, 0.44999998807907104f, 0.94999998807907104f);
        }
    case 6U:
        {

#line 774
            return float3(0.64999997615814209f, 0.34999999403953552f, 0.89999997615814209f);
        }
    default:
        {

#line 775
            return float3(0.94999998807907104f, 0.44999998807907104f, 0.80000001192092896f);
        }
    }

#line 775
}


#line 1678
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_1)
{
    return matrix<float,int(3),int(3)> (cross(basis_1[int(1)], basis_1[int(2)]), cross(basis_1[int(2)], basis_1[int(0)]), cross(basis_1[int(0)], basis_1[int(1)]));
}


#line 1062
float3 load_position_0(uint at_1, KernelContext_0 thread* kernelContext_4)
{
    uint word_0 = at_1 * 3U;
    return float3((as_type<float>((kernelContext_4->vertices_0[word_0]))), (as_type<float>((kernelContext_4->vertices_0[word_0 + 1U]))), (as_type<float>((kernelContext_4->vertices_0[word_0 + 2U]))));
}


#line 157
float dequantise_snorm_0(int lane_0)
{
    return max(float(lane_0) / 32767.0f, -1.0f);
}


float4 unpack_snorm16x4_0(uint low_0, uint high_0)
{
    return float4(dequantise_snorm_0((as_type<int>((low_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((low_0))) >> 16U), dequantise_snorm_0((as_type<int>((high_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((high_0))) >> 16U));
}


#line 189
float3 rotate_by_0(float4 q_0, float3 v_0)
{
    float3 _S10 = q_0.xyz;

#line 191
    float3 t_1 = float3(2.0f)  * cross(_S10, v_0);
    return v_0 + float3(q_0.w)  * t_1 + cross(_S10, t_1);
}


#line 147
struct TangentFrame_0
{
    float3 tangent_0;
    float3 bitangent_0;
    float3 normal_0;
};


#line 203
TangentFrame_0 decode_qtangent_0(float4 lanes_0)
{
    float4 q_1 = normalize(lanes_0);
    thread TangentFrame_0 basis_2;
    float3 _S11 = rotate_by_0(q_1, float3(1.0f, 0.0f, 0.0f));

#line 207
    (&basis_2)->tangent_0 = _S11;
    float3 _S12 = rotate_by_0(q_1, float3(0.0f, 0.0f, 1.0f));

#line 208
    (&basis_2)->normal_0 = _S12;
    float3 _S13 = cross(_S12, _S11);

#line 209
    float _S14;

#line 209
    if((lanes_0.w) < 0.0f)
    {

#line 209
        _S14 = -1.0f;

#line 209
    }
    else
    {

#line 209
        _S14 = 1.0f;

#line 209
    }

#line 209
    (&basis_2)->bitangent_0 = _S13 * float3(_S14) ;
    return basis_2;
}


#line 172
float2 unpack_unorm16x2_0(uint word_1)
{
    return float2(float(word_1 & 65535U), float(word_1 >> 16U)) / float2(65535.0f) ;
}


float4 unpack_rgba8_0(uint word_2)
{
    return float4(float(word_2 & 255U), float((word_2 >> 8U) & 255U), float((word_2 >> 16U) & 255U), float(word_2 >> 24U)) / float4(255.0f) ;
}


#line 218
struct MeshVertex_0
{
    float3 position_0;
    TangentFrame_0 basis_3;
    float2 uv0_0;
    float4 color_0;
};


#line 1073
MeshVertex_0 load_vertex_0(uint at_2, float4 range_0, KernelContext_0 thread* kernelContext_5)
{
    uint word_3 = kernelContext_5->frame_0->vertex_pool_0.x + at_2 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1076
    float3 _S15 = load_position_0(at_2, kernelContext_5);
    (&vertex_0)->position_0 = _S15;
    (&vertex_0)->basis_3 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_5->vertices_0[word_3], kernelContext_5->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_5->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_0 = unpack_rgba8_0(kernelContext_5->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1374
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_4)
{

#line 1374
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1376
        word_4 = 1U;

#line 1376
    }
    else
    {

#line 1376
        word_4 = 0U;

#line 1376
    }

    if((dot(cross(basis_4->normal_0, basis_4->tangent_0), basis_4->bitangent_0)) < 0.0f)
    {

#line 1378
        word_4 = word_4 | 2U;

#line 1378
    }

#line 1377
    return word_4;
}




uint corner_at_0(uint corner_0, KernelContext_0 thread* kernelContext_6)
{

    return (kernelContext_6->cluster_corners_0[corner_0 >> 2U] >> ((corner_0 & 3U) * 8U)) & 255U;
}


#line 1338
struct VertexOutput_0
{
    float4 position_1 [[position]];
    float3 world_position_0 [[user(CRCBL_WORLD_POSITION)]];
    float3 world_normal_0 [[user(CRCBL_WORLD_NORMAL)]];
    float4 color_1 [[user(CRCBL_COLOR)]];
    [[flat]] uint material_1 [[user(CRCBL_MATERIAL)]];
    float2 uv_0 [[user(CRCBL_UV)]];
    float4 clip_position_0 [[user(CRCBL_CLIP_POSITION)]];
    float4 previous_clip_position_0 [[user(CRCBL_PREVIOUS_CLIP_POSITION)]];
    float3 world_tangent_0 [[user(CRCBL_WORLD_TANGENT)]];
    [[flat]] uint frame_1 [[user(CRCBL_FRAME)]];
};


#line 1854
[[mesh]] void meshMain(uint3 lane_1 [[thread_position_in_threadgroup]], uint3 group_2 [[threadgroup_position_in_grid]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_1 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_1 [[buffer(14)]], Meshlet_0 device* clusters_1 [[buffer(11)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], ClusterSelect_0 device* cluster_select_1 [[buffer(17)]], uint device* tables_1 [[buffer(10)]], uint device* cluster_vertices_1 [[buffer(12)]], uint device* vertices_1 [[buffer(1)]], uint device* cluster_corners_1 [[buffer(13)]], uint device* group_state_1 [[buffer(19)]], CullParams_natural_0 constant* cull_1 [[buffer(15)]], atomic<uint> device* cull_stats_1 [[buffer(16)]], uint device* cluster_selection_1 [[buffer(18)]])
{
    thread KernelContext_0 kernelContext_7;

#line 1856
    (&kernelContext_7)->draw_0 = draw_1;

#line 1856
    (&kernelContext_7)->draw_args_0 = draw_args_1;

#line 1856
    (&kernelContext_7)->clusters_0 = clusters_1;

#line 1856
    (&kernelContext_7)->visible_instances_0 = visible_instances_1;

#line 1856
    (&kernelContext_7)->instances_0 = instances_1;

#line 1856
    (&kernelContext_7)->meshes_0 = meshes_1;

#line 1856
    (&kernelContext_7)->frame_0 = frame_2;

#line 1856
    (&kernelContext_7)->cluster_select_0 = cluster_select_1;

#line 1856
    (&kernelContext_7)->tables_0 = tables_1;

#line 1856
    (&kernelContext_7)->cluster_vertices_0 = cluster_vertices_1;

#line 1856
    (&kernelContext_7)->vertices_0 = vertices_1;

#line 1856
    (&kernelContext_7)->cluster_corners_0 = cluster_corners_1;

#line 1856
    (&kernelContext_7)->group_state_0 = group_state_1;

#line 1856
    (&kernelContext_7)->cull_0 = cull_1;

#line 1856
    (&kernelContext_7)->cull_stats_0 = cull_stats_1;

#line 1856
    (&kernelContext_7)->cluster_selection_0 = cluster_selection_1;

#line 1856
    threadgroup array<uint, int(32)> task_kept_1;

#line 1856
    (&kernelContext_7)->task_kept_0 = &task_kept_1;

#line 1856
    threadgroup ClusterPayload_0 task_payload_1;

#line 1856
    (&kernelContext_7)->task_payload_0 = &task_payload_1;

#line 1856
    uint lane_2 = lane_1.x;

#line 1856
    ClusterSource_0 _S16 = cluster_source_0(0U, &kernelContext_7);

#line 1856
    thread ClusterSource_0 _S17 = _S16;

#line 1856
    uint _S18 = group_is_live_0(group_2, &_S17, &kernelContext_7);

#line 1863
    uint _S19 = _S16.cluster_base_0 + group_2.x * _S18;

#line 1863
    uint _S20 = group_2.y;

#line 1862
    for(;;)
    {

#line 1862
        Meshlet_0 cluster_0 = (&kernelContext_7)->clusters_0[_S19];

#line 1862
        _slang_mesh.set_primitive_count((cluster_0.triangle_count_0 * _S18));

#line 1862
        if(_S18 == 0U)
        {

#line 1862
            break;
        }

#line 1862
        GpuInstance_natural_0 device* _S21 = (&kernelContext_7)->instances_0+(&kernelContext_7)->visible_instances_0[(&kernelContext_7)->visible_instances_0[_S16.start_at_0] + _S20];

#line 1862
        GpuMesh_0 mesh_1 = (&kernelContext_7)->meshes_0[_S21->mesh_0];

#line 1862
        float4 _S22 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 1862
        float4 overlay_0;

#line 1862
        if(((&kernelContext_7)->frame_0->ambient_0.w) >= 2.5f)
        {

#line 1862
            float3 _S23 = cluster_heat_0(_S19, matrix<float,int(4),int(4)> (_S21->transform_0.data_0[int(0)][int(0)], _S21->transform_0.data_0[int(1)][int(0)], _S21->transform_0.data_0[int(2)][int(0)], _S21->transform_0.data_0[int(3)][int(0)], _S21->transform_0.data_0[int(0)][int(1)], _S21->transform_0.data_0[int(1)][int(1)], _S21->transform_0.data_0[int(2)][int(1)], _S21->transform_0.data_0[int(3)][int(1)], _S21->transform_0.data_0[int(0)][int(2)], _S21->transform_0.data_0[int(1)][int(2)], _S21->transform_0.data_0[int(2)][int(2)], _S21->transform_0.data_0[int(3)][int(2)], _S21->transform_0.data_0[int(0)][int(3)], _S21->transform_0.data_0[int(1)][int(3)], _S21->transform_0.data_0[int(2)][int(3)], _S21->transform_0.data_0[int(3)][int(3)]), &kernelContext_7);

#line 1862
            overlay_0 = float4(_S23, 1.0f);

#line 1862
        }
        else
        {

#line 1862
            if(((&kernelContext_7)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1862
                overlay_0 = float4(lod_tint_0(((&kernelContext_7)->cluster_select_0[_S19].flags_2) >> 2U), 1.0f);

#line 1862
            }
            else
            {

#line 1862
                overlay_0 = _S22;

#line 1862
            }

#line 1862
        }

#line 1862
        ClusterSelect_0 _S24 = (&kernelContext_7)->cluster_select_0[_S19];

#line 1862
        bool _S25 = ((_S21->flags_0) & 2U) != 0U;

#line 1862
        uint base_vertex_2;

#line 1862
        if(_S25)
        {

#line 1862
            base_vertex_2 = _S21->base_vertex_0;

#line 1862
        }
        else
        {

#line 1862
            base_vertex_2 = mesh_1.base_vertex_1;

#line 1862
        }

#line 1862
        uint t_2;

#line 1862
        if(_S25)
        {

#line 1862
            t_2 = _S21->previous_base_vertex_0;

#line 1862
        }
        else
        {

#line 1862
            t_2 = base_vertex_2;

#line 1862
        }

#line 1862
        matrix<float,int(4),int(4)>  _S26 = matrix<float,int(4),int(4)> (_S21->transform_0.data_0[int(0)][int(0)], _S21->transform_0.data_0[int(1)][int(0)], _S21->transform_0.data_0[int(2)][int(0)], _S21->transform_0.data_0[int(3)][int(0)], _S21->transform_0.data_0[int(0)][int(1)], _S21->transform_0.data_0[int(1)][int(1)], _S21->transform_0.data_0[int(2)][int(1)], _S21->transform_0.data_0[int(3)][int(1)], _S21->transform_0.data_0[int(0)][int(2)], _S21->transform_0.data_0[int(1)][int(2)], _S21->transform_0.data_0[int(2)][int(2)], _S21->transform_0.data_0[int(3)][int(2)], _S21->transform_0.data_0[int(0)][int(3)], _S21->transform_0.data_0[int(1)][int(3)], _S21->transform_0.data_0[int(2)][int(3)], _S21->transform_0.data_0[int(3)][int(3)]);

#line 1862
        matrix<float,int(3),int(3)>  _S27 = matrix<float,int(3),int(3)> (_S26[int(0)].xyz, _S26[int(1)].xyz, _S26[int(2)].xyz);

#line 1862
        matrix<float,int(3),int(3)>  _S28 = normal_basis_0(_S27);

#line 1862
        float4 _S29 = float4(mesh_1.uv_scale_u_0, mesh_1.uv_scale_v_0, mesh_1.uv_offset_u_0, mesh_1.uv_offset_v_0);

#line 1862
        uint v_1 = lane_2;

#line 1862
        for(;;)
        {

#line 1862
            if(v_1 < (cluster_0.vertex_count_0))
            {
            }
            else
            {

#line 1862
                break;
            }

#line 1862
            uint index_0 = (&kernelContext_7)->cluster_vertices_0[cluster_0.vertex_offset_1 + v_1];

#line 1862
            MeshVertex_0 _S30 = load_vertex_0(index_0 + base_vertex_2 + _S24.vertex_base_0, _S29, &kernelContext_7);

#line 1862
            float4 world_0 = (((float4(_S30.position_0, 1.0f)) * (_S26)));

#line 1862
            thread VertexOutput_0 output_0;

#line 1862
            (&output_0)->position_1 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_7)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_7)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 1862
            (&output_0)->world_position_0 = world_0.xyz;

#line 1862
            (&output_0)->world_normal_0 = (((_S30.basis_3.normal_0) * (_S28)));

#line 1862
            (&output_0)->world_tangent_0 = (((_S30.basis_3.tangent_0) * (_S27)));

#line 1862
            thread TangentFrame_0 _S31 = _S30.basis_3;

#line 1862
            uint _S32 = frame_word_0(mesh_1.flags_1, &_S31);

#line 1862
            (&output_0)->frame_1 = _S32;

#line 1862
            float4 _S33;

#line 1862
            if(((&kernelContext_7)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1862
                _S33 = overlay_0;

#line 1862
            }
            else
            {

#line 1862
                _S33 = _S30.color_0;

#line 1862
            }

#line 1862
            (&output_0)->color_1 = _S33;

#line 1862
            (&output_0)->material_1 = _S21->material_0;

#line 1862
            (&output_0)->uv_0 = _S30.uv0_0;

#line 1862
            float3 _S34 = load_position_0(index_0 + t_2 + _S24.vertex_base_0, &kernelContext_7);

#line 1862
            (&output_0)->clip_position_0 = (&output_0)->position_1;

#line 1862
            (&output_0)->previous_clip_position_0 = ((((((float4(_S34, 1.0f)) * (matrix<float,int(4),int(4)> (_S21->previous_transform_0.data_0[int(0)][int(0)], _S21->previous_transform_0.data_0[int(1)][int(0)], _S21->previous_transform_0.data_0[int(2)][int(0)], _S21->previous_transform_0.data_0[int(3)][int(0)], _S21->previous_transform_0.data_0[int(0)][int(1)], _S21->previous_transform_0.data_0[int(1)][int(1)], _S21->previous_transform_0.data_0[int(2)][int(1)], _S21->previous_transform_0.data_0[int(3)][int(1)], _S21->previous_transform_0.data_0[int(0)][int(2)], _S21->previous_transform_0.data_0[int(1)][int(2)], _S21->previous_transform_0.data_0[int(2)][int(2)], _S21->previous_transform_0.data_0[int(3)][int(2)], _S21->previous_transform_0.data_0[int(0)][int(3)], _S21->previous_transform_0.data_0[int(1)][int(3)], _S21->previous_transform_0.data_0[int(2)][int(3)], _S21->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_7)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));

#line 1862
            _slang_mesh.set_vertex(v_1,output_0);

#line 1862
            v_1 = v_1 + 64U;

#line 1862
        }

#line 1862
        t_2 = lane_2;

#line 1862
        for(;;)
        {

#line 1862
            if(t_2 < (cluster_0.triangle_count_0))
            {
            }
            else
            {

#line 1862
                break;
            }

#line 1862
            uint corner_1 = cluster_0.triangle_offset_0 + t_2 * 3U;

#line 1862
            uint _S35 = corner_at_0(corner_1, &kernelContext_7);

#line 1862
            uint _S36 = corner_at_0(corner_1 + 1U, &kernelContext_7);

#line 1862
            uint _S37 = corner_at_0(corner_1 + 2U, &kernelContext_7);

#line 1862
            _slang_mesh.set_index(t_2*3+0,(uint3(_S35, _S36, _S37))[0]);
            _slang_mesh.set_index(t_2*3+1,(uint3(_S35, _S36, _S37))[1]);
            _slang_mesh.set_index(t_2*3+2,(uint3(_S35, _S36, _S37))[2]);
            ;

#line 1862
            t_2 = t_2 + 64U;

#line 1862
        }

#line 1862
        break;
    }
    return;
}


#line 1211
ClusterSource_0 cluster_source_at_0(uint offset_0, KernelContext_0 thread* kernelContext_8)
{
    thread ClusterSource_0 source_2;
    (&source_2)->start_at_0 = kernelContext_8->draw_0->start_at_1 + offset_0;
    (&source_2)->cluster_base_0 = kernelContext_8->tables_0[kernelContext_8->draw_0->cluster_base_at_0 + offset_0];
    (&source_2)->cluster_count_0 = kernelContext_8->tables_0[kernelContext_8->draw_0->cluster_count_at_0 + offset_0];
    (&source_2)->bucket_0 = kernelContext_8->draw_0->bucket_1 + offset_0;
    return source_2;
}


#line 1652
uint cluster_is_selected_0(const ClusterSelect_0 thread* select_1, uint instance_index_0, KernelContext_0 thread* kernelContext_9)
{
    uint base_0 = instance_index_0 * kernelContext_9->draw_0->group_stride_0;

#line 1654
    uint _S38 = select_1->flags_2;

#line 1654
    bool producer_expanded_0;


    if(((select_1->flags_2) & 1U) != 0U)
    {

#line 1657
        producer_expanded_0 = kernelContext_9->group_state_0[base_0 + select_1->producer_group_0] != 0U;

#line 1657
    }
    else
    {

#line 1657
        producer_expanded_0 = false;

#line 1657
    }

#line 1657
    bool container_expanded_0;

    if((_S38 & 2U) == 0U)
    {

#line 1659
        container_expanded_0 = true;

#line 1659
    }
    else
    {

#line 1659
        container_expanded_0 = kernelContext_9->group_state_0[base_0 + select_1->container_group_0] != 0U;

#line 1659
    }

    if(!producer_expanded_0)
    {

#line 1661
        producer_expanded_0 = container_expanded_0;

#line 1661
    }
    else
    {

#line 1661
        producer_expanded_0 = false;

#line 1661
    }

#line 1661
    uint _S39;

#line 1661
    if(producer_expanded_0)
    {

#line 1661
        _S39 = 1U;

#line 1661
    }
    else
    {

#line 1661
        _S39 = 0U;

#line 1661
    }

#line 1661
    return _S39;
}


#line 464
bool cone_may_reject_0(uint material_mode_0)
{
    return (material_mode_0 & 2U) == 0U;
}


#line 1506
bool preserves_angles_0(matrix<float,int(3),int(3)>  basis_5)
{
    matrix<float,int(3),int(3)>  gram_0 = (((basis_5) * (transpose(basis_5))));
    float _S40 = max(gram_0[int(0)][int(0)], max(gram_0[int(1)][int(1)], gram_0[int(2)][int(2)]));
    if(_S40 <= 0.0f)
    {
        return false;
    }
    float slack_0 = 0.00009999999747379f * _S40;

#line 1514
    bool _S41;
    if((abs(gram_0[int(0)][int(1)])) <= slack_0)
    {

#line 1515
        _S41 = (abs(gram_0[int(0)][int(2)])) <= slack_0;

#line 1515
    }
    else
    {

#line 1515
        _S41 = false;

#line 1515
    }

#line 1515
    if(_S41)
    {

#line 1515
        _S41 = (abs(gram_0[int(1)][int(2)])) <= slack_0;

#line 1515
    }
    else
    {

#line 1515
        _S41 = false;

#line 1515
    }
    if(_S41)
    {

#line 1516
        _S41 = (_S40 - gram_0[int(0)][int(0)]) <= slack_0;

#line 1516
    }
    else
    {

#line 1516
        _S41 = false;

#line 1516
    }

#line 1516
    if(_S41)
    {

#line 1516
        _S41 = (_S40 - gram_0[int(1)][int(1)]) <= slack_0;

#line 1516
    }
    else
    {

#line 1516
        _S41 = false;

#line 1516
    }
    if(_S41)
    {

#line 1517
        _S41 = (_S40 - gram_0[int(2)][int(2)]) <= slack_0;

#line 1517
    }
    else
    {

#line 1517
        _S41 = false;

#line 1517
    }

#line 1515
    return _S41;
}


#line 1595
uint cluster_survives_0(const Meshlet_0 thread* cluster_1, matrix<float,int(4),int(4)>  transform_2, uint material_mode_1, KernelContext_0 thread* kernelContext_10)
{
    matrix<float,int(3),int(3)>  _S42 = matrix<float,int(3),int(3)> (transform_2[int(0)].xyz, transform_2[int(1)].xyz, transform_2[int(2)].xyz);
    float3 center_1 = (((float4(cluster_1->center_x_0, cluster_1->center_y_0, cluster_1->center_z_0, 1.0f)) * (transform_2))).xyz;
    float radius_3 = cluster_1->radius_0 * max_stretch_0(_S42);

#line 1599
    uint plane_0 = 0U;

    for(;;)
    {

#line 1601
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 1601
            break;
        }

        float3 _S43 = kernelContext_10->cull_0->planes_0[plane_0].xyz;

#line 1604
        if((dot(_S43, center_1) + kernelContext_10->cull_0->planes_0[plane_0].w) < (- radius_3 * length(_S43)))
        {
            return 1U;
        }

#line 1601
        plane_0 = plane_0 + 1U;

#line 1601
    }

#line 1618
    float3 axis_0 = (((float3(cluster_1->cone_axis_x_0, cluster_1->cone_axis_y_0, cluster_1->cone_axis_z_0)) * (_S42)));

    float axis_length_0 = length(axis_0);

#line 1620
    float3 axis_1;
    if(axis_length_0 > 0.0f)
    {

#line 1621
        axis_1 = axis_0 / float3(axis_length_0) ;

#line 1621
    }
    else
    {

#line 1621
        axis_1 = float3(0.0f, 0.0f, 0.0f);

#line 1621
    }
    float3 to_center_0 = center_1 - kernelContext_10->frame_0->camera_position_0.xyz;

#line 1622
    float _S44 = cluster_1->cone_cutoff_0;
    float sine_0 = sqrt(max(0.0f, 1.0f - cluster_1->cone_cutoff_0 * cluster_1->cone_cutoff_0));

#line 1623
    bool _S45;
    if(cone_may_reject_0(material_mode_1))
    {

#line 1624
        _S45 = preserves_angles_0(_S42);

#line 1624
    }
    else
    {

#line 1624
        _S45 = false;

#line 1624
    }

#line 1624
    if(_S45)
    {

#line 1624
        _S45 = _S44 > 0.0f;

#line 1624
    }
    else
    {

#line 1624
        _S45 = false;

#line 1624
    }
    if(_S45)
    {

#line 1625
        _S45 = (dot(axis_1, to_center_0)) > (sine_0 * length(to_center_0) + radius_3);

#line 1625
    }
    else
    {

#line 1625
        _S45 = false;

#line 1625
    }

#line 1624
    if(_S45)
    {

        return 2U;
    }

    return 0U;
}


#line 11866 "hlsl.meta.slang"
uint instance_material_mode_0(uint _S46, KernelContext_0 thread* kernelContext_11)
{

#line 441 "shaders/mesh_cluster.slang"
    return (((kernelContext_11->instances_0+_S46)->flags_0) & 12U) >> 2U;
}


#line 1946
[[object]] void taskMain(uint3 lane_3 [[thread_position_in_threadgroup]], uint3 chunk_group_0 [[threadgroup_position_in_grid]], ClusterPayload_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp, ClusterDrawConstants_0 constant* draw_2 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_2 [[buffer(14)]], Meshlet_0 device* clusters_2 [[buffer(11)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], ClusterSelect_0 device* cluster_select_2 [[buffer(17)]], uint device* tables_2 [[buffer(10)]], uint device* cluster_vertices_2 [[buffer(12)]], uint device* vertices_2 [[buffer(1)]], uint device* cluster_corners_2 [[buffer(13)]], uint device* group_state_2 [[buffer(19)]], CullParams_natural_0 constant* cull_2 [[buffer(15)]], atomic<uint> device* cull_stats_2 [[buffer(16)]], uint device* cluster_selection_2 [[buffer(18)]])
{

#line 1946
    thread KernelContext_0 kernelContext_12;

#line 1946
    (&kernelContext_12)->draw_0 = draw_2;

#line 1946
    (&kernelContext_12)->draw_args_0 = draw_args_2;

#line 1946
    (&kernelContext_12)->clusters_0 = clusters_2;

#line 1946
    (&kernelContext_12)->visible_instances_0 = visible_instances_2;

#line 1946
    (&kernelContext_12)->instances_0 = instances_2;

#line 1946
    (&kernelContext_12)->meshes_0 = meshes_2;

#line 1946
    (&kernelContext_12)->frame_0 = frame_3;

#line 1946
    (&kernelContext_12)->cluster_select_0 = cluster_select_2;

#line 1946
    (&kernelContext_12)->tables_0 = tables_2;

#line 1946
    (&kernelContext_12)->cluster_vertices_0 = cluster_vertices_2;

#line 1946
    (&kernelContext_12)->vertices_0 = vertices_2;

#line 1946
    (&kernelContext_12)->cluster_corners_0 = cluster_corners_2;

#line 1946
    (&kernelContext_12)->group_state_0 = group_state_2;

#line 1946
    (&kernelContext_12)->cull_0 = cull_2;

#line 1946
    (&kernelContext_12)->cull_stats_0 = cull_stats_2;

#line 1946
    (&kernelContext_12)->cluster_selection_0 = cluster_selection_2;

#line 1946
    threadgroup array<uint, int(32)> task_kept_2;

#line 1946
    (&kernelContext_12)->task_kept_0 = &task_kept_2;

#line 1946
    threadgroup ClusterPayload_0 task_payload_2;

#line 1946
    (&kernelContext_12)->task_payload_0 = &task_payload_2;

#line 1946
    uint lane_4 = lane_3.x;

#line 1961
    uint target_0 = visible_instances_2[draw_2->chunk_starts_at_0] + (chunk_group_0.y * 65535U + chunk_group_0.x);

#line 1961
    uint limit_0 = draw_2->chunk_starts_end_0 - draw_2->chunk_starts_at_0;

#line 1961
    uint offset_1 = 0U;


    for(;;)
    {

#line 1964
        if((limit_0 - offset_1) > 1U)
        {
        }
        else
        {

#line 1964
            break;
        }
        uint middle_0 = (offset_1 + limit_0) / 2U;
        if((&kernelContext_12)->visible_instances_0[draw_2->chunk_starts_at_0 + middle_0] <= target_0)
        {

#line 1967
            offset_1 = middle_0;

#line 1967
        }
        else
        {

#line 1967
            limit_0 = middle_0;

#line 1967
        }

#line 1964
    }

#line 1964
    ClusterSource_0 _S47 = cluster_source_at_0(offset_1, &kernelContext_12);

#line 1978
    uint pair_0 = (target_0 - (&kernelContext_12)->visible_instances_0[draw_2->chunk_starts_at_0 + offset_1]) * 32U + lane_4;



    uint _S48 = max(_S47.cluster_count_0, 1U);


    uint _S49 = pair_0 % _S48;

#line 1985
    uint _S50 = pair_0 / _S48;

#line 1985
    uint3 group_3 = uint3(_S49, _S50, 0U);

#line 1985
    thread ClusterSource_0 _S51 = _S47;

#line 1985
    uint _S52 = group_is_live_0(group_3, &_S51, &kernelContext_12);

    uint _S53 = _S47.cluster_base_0 + _S49 * _S52;

#line 1987
    Meshlet_0 cluster_2 = (&kernelContext_12)->clusters_0[_S53];

    uint instance_index_1 = (&kernelContext_12)->visible_instances_0[(&kernelContext_12)->visible_instances_0[_S47.start_at_0] + _S50 * _S52] * _S52;

#line 1989
    GpuInstance_natural_0 device* _S54 = (&kernelContext_12)->instances_0+instance_index_1;

#line 1989
    thread ClusterSelect_0 _S55 = (&kernelContext_12)->cluster_select_0[_S53];

#line 1989
    uint _S56 = cluster_is_selected_0(&_S55, instance_index_1, &kernelContext_12);

#line 1989
    uint verdict_0;

#line 1997
    if(((_S54->flags_0) & 2U) != 0U)
    {

#line 1997
        verdict_0 = 0U;

#line 1997
    }
    else
    {

#line 1997
        matrix<float,int(4),int(4)>  _S57 = matrix<float,int(4),int(4)> (_S54->transform_0.data_0[int(0)][int(0)], _S54->transform_0.data_0[int(1)][int(0)], _S54->transform_0.data_0[int(2)][int(0)], _S54->transform_0.data_0[int(3)][int(0)], _S54->transform_0.data_0[int(0)][int(1)], _S54->transform_0.data_0[int(1)][int(1)], _S54->transform_0.data_0[int(2)][int(1)], _S54->transform_0.data_0[int(3)][int(1)], _S54->transform_0.data_0[int(0)][int(2)], _S54->transform_0.data_0[int(1)][int(2)], _S54->transform_0.data_0[int(2)][int(2)], _S54->transform_0.data_0[int(3)][int(2)], _S54->transform_0.data_0[int(0)][int(3)], _S54->transform_0.data_0[int(1)][int(3)], _S54->transform_0.data_0[int(2)][int(3)], _S54->transform_0.data_0[int(3)][int(3)]);

#line 1997
        uint _S58 = instance_material_mode_0(instance_index_1, &kernelContext_12);

#line 1997
        thread Meshlet_0 _S59 = cluster_2;

#line 1997
        uint _S60 = cluster_survives_0(&_S59, _S57, _S58, &kernelContext_12);

#line 1997
        verdict_0 = _S60;

#line 1997
    }

    uint _S61 = _S52 * _S56;

#line 1999
    bool _S62 = verdict_0 == 0U;

#line 1999
    if(_S62)
    {

#line 1999
        limit_0 = 1U;

#line 1999
    }
    else
    {

#line 1999
        limit_0 = 0U;

#line 1999
    }

#line 1999
    uint keep_0 = _S61 * limit_0;

#line 1999
    uint word_5;

#line 2012
    if(_S62)
    {

#line 2012
        word_5 = 1U;

#line 2012
    }
    else
    {

#line 2013
        if(verdict_0 == 1U)
        {

#line 2013
            limit_0 = 3U;

#line 2013
        }
        else
        {

#line 2013
            limit_0 = 4U;

#line 2013
        }

#line 2013
        word_5 = limit_0;

#line 2012
    }


    if(_S61 == 1U)
    {
        uint _S63 = atomic_fetch_add_explicit((&kernelContext_12)->cull_stats_0+word_5, 1U, memory_order_relaxed);

#line 2015
    }

#line 2015
    bool _S64;

#line 2030
    if(_S50 == 0U)
    {

#line 2030
        _S64 = _S52 == 1U;

#line 2030
    }
    else
    {

#line 2030
        _S64 = false;

#line 2030
    }

#line 2030
    if(_S64)
    {
        *((&kernelContext_12)->cluster_selection_0+_S53) = _S56;

#line 2030
    }

#line 2039
    (*(&kernelContext_12)->task_kept_0)[lane_4] = keep_0;
    threadgroup_barrier(mem_flags::mem_threadgroup);

#line 2040
    uint other_0 = 0U;

#line 2040
    uint rank_0 = 0U;

#line 2040
    uint kept_0 = 0U;


    for(;;)
    {

#line 2043
        if(other_0 < 32U)
        {
        }
        else
        {

#line 2043
            break;
        }
        uint flag_0 = (*(&kernelContext_12)->task_kept_0)[other_0];
        if(other_0 < lane_4)
        {

#line 2046
            limit_0 = flag_0;

#line 2046
        }
        else
        {

#line 2046
            limit_0 = 0U;

#line 2046
        }

#line 2046
        uint rank_1 = rank_0 + limit_0;
        uint kept_1 = kept_0 + flag_0;

#line 2043
        other_0 = other_0 + 1U;

#line 2043
        rank_0 = rank_1;

#line 2043
        kept_0 = kept_1;

#line 2043
    }

#line 2049
    if(keep_0 == 1U)
    {
        (&kernelContext_12)->task_payload_0->pairs_0[rank_0] = pair_0;

#line 2049
    }



    if(lane_4 == 0U)
    {
        (&kernelContext_12)->task_payload_0->bucket_offset_0 = offset_1;

#line 2053
    }

#line 2058
    threadgroup_barrier(mem_flags::mem_threadgroup);
    *_slang_mesh_payload = *((&kernelContext_12)->task_payload_0); _slang_mgp.set_threadgroups_per_grid(uint3((kept_0), (1U), (1U))); return;;
    return;
}


#line 2071
[[mesh]] void amplifiedMeshMain(uint3 lane_5 [[thread_position_in_threadgroup]], uint3 group_4 [[threadgroup_position_in_grid]], const ClusterPayload_0 object_data* amplification_0 [[payload]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_3 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_3 [[buffer(14)]], Meshlet_0 device* clusters_3 [[buffer(11)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_4 [[buffer(0)]], ClusterSelect_0 device* cluster_select_3 [[buffer(17)]], uint device* tables_3 [[buffer(10)]], uint device* cluster_vertices_3 [[buffer(12)]], uint device* vertices_3 [[buffer(1)]], uint device* cluster_corners_3 [[buffer(13)]], uint device* group_state_3 [[buffer(19)]], CullParams_natural_0 constant* cull_3 [[buffer(15)]], atomic<uint> device* cull_stats_3 [[buffer(16)]], uint device* cluster_selection_3 [[buffer(18)]])
{

    thread KernelContext_0 kernelContext_13;

#line 2074
    (&kernelContext_13)->draw_0 = draw_3;

#line 2074
    (&kernelContext_13)->draw_args_0 = draw_args_3;

#line 2074
    (&kernelContext_13)->clusters_0 = clusters_3;

#line 2074
    (&kernelContext_13)->visible_instances_0 = visible_instances_3;

#line 2074
    (&kernelContext_13)->instances_0 = instances_3;

#line 2074
    (&kernelContext_13)->meshes_0 = meshes_3;

#line 2074
    (&kernelContext_13)->frame_0 = frame_4;

#line 2074
    (&kernelContext_13)->cluster_select_0 = cluster_select_3;

#line 2074
    (&kernelContext_13)->tables_0 = tables_3;

#line 2074
    (&kernelContext_13)->cluster_vertices_0 = cluster_vertices_3;

#line 2074
    (&kernelContext_13)->vertices_0 = vertices_3;

#line 2074
    (&kernelContext_13)->cluster_corners_0 = cluster_corners_3;

#line 2074
    (&kernelContext_13)->group_state_0 = group_state_3;

#line 2074
    (&kernelContext_13)->cull_0 = cull_3;

#line 2074
    (&kernelContext_13)->cull_stats_0 = cull_stats_3;

#line 2074
    (&kernelContext_13)->cluster_selection_0 = cluster_selection_3;

#line 2074
    threadgroup array<uint, int(32)> task_kept_3;

#line 2074
    (&kernelContext_13)->task_kept_0 = &task_kept_3;

#line 2074
    threadgroup ClusterPayload_0 task_payload_3;

#line 2074
    (&kernelContext_13)->task_payload_0 = &task_payload_3;

#line 2074
    uint lane_6 = lane_5.x;

#line 2074
    ClusterSource_0 _S65 = cluster_source_at_0(amplification_0->bucket_offset_0, &kernelContext_13);

#line 2079
    uint pair_1 = amplification_0->pairs_0[group_4.x];


    uint _S66 = max(_S65.cluster_count_0, 1U);


    uint _S67 = amplification_0->pairs_0[group_4.x] % _S66;

#line 2085
    uint _S68 = _S65.cluster_base_0 + _S67;
    uint _S69 = pair_1 / _S66;

#line 2083
    for(;;)
    {

#line 2083
        Meshlet_0 cluster_3 = (&kernelContext_13)->clusters_0[_S68];

#line 2083
        _slang_mesh.set_primitive_count((cluster_3.triangle_count_0));

#line 2083
        GpuInstance_natural_0 device* _S70 = (&kernelContext_13)->instances_0+(&kernelContext_13)->visible_instances_0[(&kernelContext_13)->visible_instances_0[_S65.start_at_0] + _S69];

#line 2083
        GpuMesh_0 mesh_2 = (&kernelContext_13)->meshes_0[_S70->mesh_0];

#line 2083
        float4 _S71 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 2083
        float4 overlay_1;

#line 2083
        if(((&kernelContext_13)->frame_0->ambient_0.w) >= 2.5f)
        {

#line 2083
            float3 _S72 = cluster_heat_0(_S68, matrix<float,int(4),int(4)> (_S70->transform_0.data_0[int(0)][int(0)], _S70->transform_0.data_0[int(1)][int(0)], _S70->transform_0.data_0[int(2)][int(0)], _S70->transform_0.data_0[int(3)][int(0)], _S70->transform_0.data_0[int(0)][int(1)], _S70->transform_0.data_0[int(1)][int(1)], _S70->transform_0.data_0[int(2)][int(1)], _S70->transform_0.data_0[int(3)][int(1)], _S70->transform_0.data_0[int(0)][int(2)], _S70->transform_0.data_0[int(1)][int(2)], _S70->transform_0.data_0[int(2)][int(2)], _S70->transform_0.data_0[int(3)][int(2)], _S70->transform_0.data_0[int(0)][int(3)], _S70->transform_0.data_0[int(1)][int(3)], _S70->transform_0.data_0[int(2)][int(3)], _S70->transform_0.data_0[int(3)][int(3)]), &kernelContext_13);

#line 2083
            overlay_1 = float4(_S72, 1.0f);

#line 2083
        }
        else
        {

#line 2083
            if(((&kernelContext_13)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 2083
                overlay_1 = float4(lod_tint_0(((&kernelContext_13)->cluster_select_0[_S68].flags_2) >> 2U), 1.0f);

#line 2083
            }
            else
            {

#line 2083
                overlay_1 = _S71;

#line 2083
            }

#line 2083
        }

#line 2083
        ClusterSelect_0 _S73 = (&kernelContext_13)->cluster_select_0[_S68];

#line 2083
        bool _S74 = ((_S70->flags_0) & 2U) != 0U;

#line 2083
        uint base_vertex_3;

#line 2083
        if(_S74)
        {

#line 2083
            base_vertex_3 = _S70->base_vertex_0;

#line 2083
        }
        else
        {

#line 2083
            base_vertex_3 = mesh_2.base_vertex_1;

#line 2083
        }

#line 2083
        uint t_3;

#line 2083
        if(_S74)
        {

#line 2083
            t_3 = _S70->previous_base_vertex_0;

#line 2083
        }
        else
        {

#line 2083
            t_3 = base_vertex_3;

#line 2083
        }

#line 2083
        matrix<float,int(4),int(4)>  _S75 = matrix<float,int(4),int(4)> (_S70->transform_0.data_0[int(0)][int(0)], _S70->transform_0.data_0[int(1)][int(0)], _S70->transform_0.data_0[int(2)][int(0)], _S70->transform_0.data_0[int(3)][int(0)], _S70->transform_0.data_0[int(0)][int(1)], _S70->transform_0.data_0[int(1)][int(1)], _S70->transform_0.data_0[int(2)][int(1)], _S70->transform_0.data_0[int(3)][int(1)], _S70->transform_0.data_0[int(0)][int(2)], _S70->transform_0.data_0[int(1)][int(2)], _S70->transform_0.data_0[int(2)][int(2)], _S70->transform_0.data_0[int(3)][int(2)], _S70->transform_0.data_0[int(0)][int(3)], _S70->transform_0.data_0[int(1)][int(3)], _S70->transform_0.data_0[int(2)][int(3)], _S70->transform_0.data_0[int(3)][int(3)]);

#line 2083
        matrix<float,int(3),int(3)>  _S76 = matrix<float,int(3),int(3)> (_S75[int(0)].xyz, _S75[int(1)].xyz, _S75[int(2)].xyz);

#line 2083
        matrix<float,int(3),int(3)>  _S77 = normal_basis_0(_S76);

#line 2083
        float4 _S78 = float4(mesh_2.uv_scale_u_0, mesh_2.uv_scale_v_0, mesh_2.uv_offset_u_0, mesh_2.uv_offset_v_0);

#line 2083
        uint v_2 = lane_6;

#line 2083
        for(;;)
        {

#line 2083
            if(v_2 < (cluster_3.vertex_count_0))
            {
            }
            else
            {

#line 2083
                break;
            }

#line 2083
            uint index_1 = (&kernelContext_13)->cluster_vertices_0[cluster_3.vertex_offset_1 + v_2];

#line 2083
            MeshVertex_0 _S79 = load_vertex_0(index_1 + base_vertex_3 + _S73.vertex_base_0, _S78, &kernelContext_13);

#line 2083
            float4 world_1 = (((float4(_S79.position_0, 1.0f)) * (_S75)));

#line 2083
            thread VertexOutput_0 output_1;

#line 2083
            (&output_1)->position_1 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 2083
            (&output_1)->world_position_0 = world_1.xyz;

#line 2083
            (&output_1)->world_normal_0 = (((_S79.basis_3.normal_0) * (_S77)));

#line 2083
            (&output_1)->world_tangent_0 = (((_S79.basis_3.tangent_0) * (_S76)));

#line 2083
            thread TangentFrame_0 _S80 = _S79.basis_3;

#line 2083
            uint _S81 = frame_word_0(mesh_2.flags_1, &_S80);

#line 2083
            (&output_1)->frame_1 = _S81;

#line 2083
            float4 _S82;

#line 2083
            if(((&kernelContext_13)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 2083
                _S82 = overlay_1;

#line 2083
            }
            else
            {

#line 2083
                _S82 = _S79.color_0;

#line 2083
            }

#line 2083
            (&output_1)->color_1 = _S82;

#line 2083
            (&output_1)->material_1 = _S70->material_0;

#line 2083
            (&output_1)->uv_0 = _S79.uv0_0;

#line 2083
            float3 _S83 = load_position_0(index_1 + t_3 + _S73.vertex_base_0, &kernelContext_13);

#line 2083
            (&output_1)->clip_position_0 = (&output_1)->position_1;

#line 2083
            (&output_1)->previous_clip_position_0 = ((((((float4(_S83, 1.0f)) * (matrix<float,int(4),int(4)> (_S70->previous_transform_0.data_0[int(0)][int(0)], _S70->previous_transform_0.data_0[int(1)][int(0)], _S70->previous_transform_0.data_0[int(2)][int(0)], _S70->previous_transform_0.data_0[int(3)][int(0)], _S70->previous_transform_0.data_0[int(0)][int(1)], _S70->previous_transform_0.data_0[int(1)][int(1)], _S70->previous_transform_0.data_0[int(2)][int(1)], _S70->previous_transform_0.data_0[int(3)][int(1)], _S70->previous_transform_0.data_0[int(0)][int(2)], _S70->previous_transform_0.data_0[int(1)][int(2)], _S70->previous_transform_0.data_0[int(2)][int(2)], _S70->previous_transform_0.data_0[int(3)][int(2)], _S70->previous_transform_0.data_0[int(0)][int(3)], _S70->previous_transform_0.data_0[int(1)][int(3)], _S70->previous_transform_0.data_0[int(2)][int(3)], _S70->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_13)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));

#line 2083
            _slang_mesh.set_vertex(v_2,output_1);

#line 2083
            v_2 = v_2 + 64U;

#line 2083
        }

#line 2083
        t_3 = lane_6;

#line 2083
        for(;;)
        {

#line 2083
            if(t_3 < (cluster_3.triangle_count_0))
            {
            }
            else
            {

#line 2083
                break;
            }

#line 2083
            uint corner_2 = cluster_3.triangle_offset_0 + t_3 * 3U;

#line 2083
            uint _S84 = corner_at_0(corner_2, &kernelContext_13);

#line 2083
            uint _S85 = corner_at_0(corner_2 + 1U, &kernelContext_13);

#line 2083
            uint _S86 = corner_at_0(corner_2 + 2U, &kernelContext_13);

#line 2083
            _slang_mesh.set_index(t_3*3+0,(uint3(_S84, _S85, _S86))[0]);
            _slang_mesh.set_index(t_3*3+1,(uint3(_S84, _S85, _S86))[1]);
            _slang_mesh.set_index(t_3*3+2,(uint3(_S84, _S85, _S86))[2]);
            ;

#line 2083
            t_3 = t_3 + 64U;

#line 2083
        }

#line 2083
        break;
    }

#line 2091
    return;
}

