#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 777 "shaders/mesh_cluster.slang"
struct ClusterPayload_0
{
    uint cluster_0;
    uint instance_0;
};


#line 794
struct ClusterDrawConstants_0
{
    uint base_0;
    uint cluster_base_0;
    uint cluster_count_0;
    uint bucket_0;
    uint group_stride_0;
    uint level_groups_at_0;
};


#line 716
struct DrawIndexedArgs_0
{
    uint index_count_0;
    uint instance_count_0;
    uint first_index_0;
    int vertex_offset_0;
    uint first_instance_0;
};


#line 413
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


#line 1432
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1432
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


#line 375
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
};


#line 1433
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1433
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 225
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(14)> data_3;
};


#line 225
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
    float4 probe_origin_0;
    float4 probe_inv_spacing_0;
    uint4 probe_counts_0;
    float4 lod_params_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sky_sh_r_0;
    float4 sky_sh_g_0;
    float4 sky_sh_b_0;
    _MatrixStorage_float4x4_ColMajornatural_1 previous_view_proj_0;
    uint4 vertex_pool_0;
};


#line 692
struct ClusterSelect_0
{
    uint flags_1;
    uint vertex_base_0;
    uint producer_group_0;
    uint container_group_0;
};


#line 736
struct CullParams_0
{
    array<float4, int(6)> planes_0;
    uint instance_count_1;
    uint capacity_0;
};


#line 1623
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
    CullParams_0 constant* cull_0;
    atomic<uint> device* cull_stats_0;
    uint device* cluster_selection_0;
};


#line 1184
uint group_is_live_0(uint3 group_0, KernelContext_0 thread* kernelContext_0)
{

    uint _S1 = group_0.y;
    uint _S2 = group_0.x;

#line 1187
    return min(1U, max(kernelContext_0->draw_args_0[kernelContext_0->draw_0->bucket_0].instance_count_0, _S1) - _S1) * min(1U, max(kernelContext_0->draw_0->cluster_count_0, _S2) - _S2);
}


#line 839
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_1;
    float center_y_1;
    float center_z_1;
    float radius_1;
};


#line 1111
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


#line 637
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S3 = (((basis_0) * (transpose(basis_0))));

#line 639
    float bound_0 = 0.0f;

#line 639
    uint row_0 = 0U;

    for(;;)
    {

#line 641
        if(row_0 < 3U)
        {
        }
        else
        {

#line 641
            break;
        }
        float _S4 = max(bound_0, abs(_S3[row_0][int(0)]) + abs(_S3[row_0][int(1)]) + abs(_S3[row_0][int(2)]));

#line 641
        uint row_1 = row_0 + 1U;

#line 641
        bound_0 = _S4;

#line 641
        row_0 = row_1;

#line 641
    }



    return sqrt(bound_0);
}


#line 596
float projected_error_0(float error_1, float3 center_0, float radius_2, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S5 = delta_0.x;

#line 599
    float _S6 = delta_0.y;

#line 599
    float _S7 = delta_0.z;
    float distance_0 = sqrt(_S5 * _S5 + _S6 * _S6 + _S7 * _S7) - radius_2;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 555
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


#line 1152
float3 cluster_heat_0(uint cluster_index_0, matrix<float,int(4),int(4)>  transform_1, KernelContext_0 thread* kernelContext_2)
{
    ClusterSelect_0 select_0 = kernelContext_2->cluster_select_0[cluster_index_0];

#line 1154
    float projected_1;

    if(((select_0.flags_1) & 1U) != 0U)
    {

#line 1156
        LevelGroup_0 _S9 = level_group_at_0(select_0.producer_group_0, kernelContext_2);


        float stretch_0 = max_stretch_0(matrix<float,int(3),int(3)> (transform_1[int(0)].xyz, transform_1[int(1)].xyz, transform_1[int(2)].xyz));

#line 1159
        projected_1 = projected_error_0(_S9.error_0 * stretch_0, (((float4(_S9.center_x_1, _S9.center_y_1, _S9.center_z_1, 1.0f)) * (transform_1))).xyz, _S9.radius_1 * stretch_0, kernelContext_2->frame_0->camera_position_0.xyz, kernelContext_2->frame_0->lod_params_0.x);

#line 1156
    }
    else
    {

#line 1156
        projected_1 = 0.0f;

#line 1156
    }

#line 1165
    return heat_tint_0(projected_1, kernelContext_2->frame_0->lod_params_0.y, kernelContext_2->frame_0->lod_params_0.z);
}


#line 661
float3 lod_tint_0(uint level_1)
{
    switch(level_1 % 8U)
    {
    case 0U:
        {

#line 665
            return float3(0.89999997615814209f, 0.25f, 0.25f);
        }
    case 1U:
        {

#line 666
            return float3(0.94999998807907104f, 0.60000002384185791f, 0.20000000298023224f);
        }
    case 2U:
        {

#line 667
            return float3(0.89999997615814209f, 0.89999997615814209f, 0.25f);
        }
    case 3U:
        {

#line 668
            return float3(0.30000001192092896f, 0.85000002384185791f, 0.34999999403953552f);
        }
    case 4U:
        {

#line 669
            return float3(0.25f, 0.80000001192092896f, 0.85000002384185791f);
        }
    case 5U:
        {

#line 670
            return float3(0.30000001192092896f, 0.44999998807907104f, 0.94999998807907104f);
        }
    case 6U:
        {

#line 671
            return float3(0.64999997615814209f, 0.34999999403953552f, 0.89999997615814209f);
        }
    default:
        {

#line 672
            return float3(0.94999998807907104f, 0.44999998807907104f, 0.80000001192092896f);
        }
    }

#line 672
}


#line 1384
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_1)
{
    return matrix<float,int(3),int(3)> (cross(basis_1[int(1)], basis_1[int(2)]), cross(basis_1[int(2)], basis_1[int(0)]), cross(basis_1[int(0)], basis_1[int(1)]));
}


#line 891
float3 load_position_0(uint at_1, KernelContext_0 thread* kernelContext_3)
{
    uint word_0 = at_1 * 3U;
    return float3((as_type<float>((kernelContext_3->vertices_0[word_0]))), (as_type<float>((kernelContext_3->vertices_0[word_0 + 1U]))), (as_type<float>((kernelContext_3->vertices_0[word_0 + 2U]))));
}


#line 138
float dequantise_snorm_0(int lane_0)
{
    return max(float(lane_0) / 32767.0f, -1.0f);
}


float4 unpack_snorm16x4_0(uint low_0, uint high_0)
{
    return float4(dequantise_snorm_0((as_type<int>((low_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((low_0))) >> 16U), dequantise_snorm_0((as_type<int>((high_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((high_0))) >> 16U));
}


#line 170
float3 rotate_by_0(float4 q_0, float3 v_0)
{
    float3 _S10 = q_0.xyz;

#line 172
    float3 t_1 = float3(2.0f)  * cross(_S10, v_0);
    return v_0 + float3(q_0.w)  * t_1 + cross(_S10, t_1);
}


#line 128
struct TangentFrame_0
{
    float3 tangent_0;
    float3 bitangent_0;
    float3 normal_0;
};


#line 184
TangentFrame_0 decode_qtangent_0(float4 lanes_0)
{
    float4 q_1 = normalize(lanes_0);
    thread TangentFrame_0 basis_2;
    float3 _S11 = rotate_by_0(q_1, float3(1.0f, 0.0f, 0.0f));

#line 188
    (&basis_2)->tangent_0 = _S11;
    float3 _S12 = rotate_by_0(q_1, float3(0.0f, 0.0f, 1.0f));

#line 189
    (&basis_2)->normal_0 = _S12;
    float3 _S13 = cross(_S12, _S11);

#line 190
    float _S14;

#line 190
    if((lanes_0.w) < 0.0f)
    {

#line 190
        _S14 = -1.0f;

#line 190
    }
    else
    {

#line 190
        _S14 = 1.0f;

#line 190
    }

#line 190
    (&basis_2)->bitangent_0 = _S13 * float3(_S14) ;
    return basis_2;
}


#line 153
float2 unpack_unorm16x2_0(uint word_1)
{
    return float2(float(word_1 & 65535U), float(word_1 >> 16U)) / float2(65535.0f) ;
}


float4 unpack_rgba8_0(uint word_2)
{
    return float4(float(word_2 & 255U), float((word_2 >> 8U) & 255U), float((word_2 >> 16U) & 255U), float(word_2 >> 24U)) / float4(255.0f) ;
}


#line 199
struct MeshVertex_0
{
    float3 position_0;
    TangentFrame_0 basis_3;
    float2 uv0_0;
    float4 color_0;
};


#line 902
MeshVertex_0 load_vertex_0(uint at_2, float4 range_0, KernelContext_0 thread* kernelContext_4)
{
    uint word_3 = kernelContext_4->frame_0->vertex_pool_0.x + at_2 * 5U;
    thread MeshVertex_0 vertex_0;

#line 905
    float3 _S15 = load_position_0(at_2, kernelContext_4);
    (&vertex_0)->position_0 = _S15;
    (&vertex_0)->basis_3 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_4->vertices_0[word_3], kernelContext_4->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_4->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_0 = unpack_rgba8_0(kernelContext_4->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1097
uint corner_at_0(uint corner_0, KernelContext_0 thread* kernelContext_5)
{

    return (kernelContext_5->cluster_corners_0[corner_0 >> 2U] >> ((corner_0 & 3U) * 8U)) & 255U;
}


#line 1078
struct VertexOutput_0
{
    float4 position_1 [[position]];
    float3 world_position_0 [[user(POSITION0)]];
    float3 world_normal_0 [[user(NORMAL0)]];
    float4 color_1 [[user(COLOR0)]];
    [[flat]] uint material_1 [[user(TEXCOORD0)]];
    float2 uv_0 [[user(TEXCOORD1)]];
    float4 clip_position_0 [[user(TEXCOORD2)]];
    float4 previous_clip_position_0 [[user(TEXCOORD3)]];
};


#line 1548
[[mesh]] void meshMain(uint3 lane_1 [[thread_position_in_threadgroup]], uint3 group_2 [[threadgroup_position_in_grid]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_1 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_1 [[buffer(10)]], Meshlet_0 device* clusters_1 [[buffer(7)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], ClusterSelect_0 device* cluster_select_1 [[buffer(13)]], uint device* tables_1 [[buffer(19)]], uint device* cluster_vertices_1 [[buffer(8)]], uint device* vertices_1 [[buffer(1)]], uint device* cluster_corners_1 [[buffer(9)]], uint device* group_state_1 [[buffer(15)]], CullParams_0 constant* cull_1 [[buffer(11)]], atomic<uint> device* cull_stats_1 [[buffer(12)]], uint device* cluster_selection_1 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_6;

#line 1550
    (&kernelContext_6)->draw_0 = draw_1;

#line 1550
    (&kernelContext_6)->draw_args_0 = draw_args_1;

#line 1550
    (&kernelContext_6)->clusters_0 = clusters_1;

#line 1550
    (&kernelContext_6)->visible_instances_0 = visible_instances_1;

#line 1550
    (&kernelContext_6)->instances_0 = instances_1;

#line 1550
    (&kernelContext_6)->meshes_0 = meshes_1;

#line 1550
    (&kernelContext_6)->frame_0 = frame_1;

#line 1550
    (&kernelContext_6)->cluster_select_0 = cluster_select_1;

#line 1550
    (&kernelContext_6)->tables_0 = tables_1;

#line 1550
    (&kernelContext_6)->cluster_vertices_0 = cluster_vertices_1;

#line 1550
    (&kernelContext_6)->vertices_0 = vertices_1;

#line 1550
    (&kernelContext_6)->cluster_corners_0 = cluster_corners_1;

#line 1550
    (&kernelContext_6)->group_state_0 = group_state_1;

#line 1550
    (&kernelContext_6)->cull_0 = cull_1;

#line 1550
    (&kernelContext_6)->cull_stats_0 = cull_stats_1;

#line 1550
    (&kernelContext_6)->cluster_selection_0 = cluster_selection_1;

#line 1550
    uint lane_2 = lane_1.x;

#line 1550
    uint _S16 = group_is_live_0(group_2, &kernelContext_6);

#line 1555
    uint _S17 = (&kernelContext_6)->draw_0->cluster_base_0 + group_2.x * _S16;

#line 1555
    uint _S18 = group_2.y;

#line 1555
    for(;;)
    {

#line 1555
        Meshlet_0 cluster_1 = (&kernelContext_6)->clusters_0[_S17];

#line 1555
        _slang_mesh.set_primitive_count((cluster_1.triangle_count_0 * _S16));

#line 1555
        if(_S16 == 0U)
        {

#line 1555
            break;
        }

#line 1555
        GpuInstance_natural_0 device* _S19 = (&kernelContext_6)->instances_0+(&kernelContext_6)->visible_instances_0[(&kernelContext_6)->draw_0->base_0 + _S18];

#line 1555
        GpuMesh_0 mesh_1 = (&kernelContext_6)->meshes_0[_S19->mesh_0];

#line 1555
        float4 _S20 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 1555
        float4 overlay_0;

#line 1555
        if(((&kernelContext_6)->frame_0->ambient_0.w) >= 2.5f)
        {

#line 1555
            float3 _S21 = cluster_heat_0(_S17, matrix<float,int(4),int(4)> (_S19->transform_0.data_0[int(0)][int(0)], _S19->transform_0.data_0[int(1)][int(0)], _S19->transform_0.data_0[int(2)][int(0)], _S19->transform_0.data_0[int(3)][int(0)], _S19->transform_0.data_0[int(0)][int(1)], _S19->transform_0.data_0[int(1)][int(1)], _S19->transform_0.data_0[int(2)][int(1)], _S19->transform_0.data_0[int(3)][int(1)], _S19->transform_0.data_0[int(0)][int(2)], _S19->transform_0.data_0[int(1)][int(2)], _S19->transform_0.data_0[int(2)][int(2)], _S19->transform_0.data_0[int(3)][int(2)], _S19->transform_0.data_0[int(0)][int(3)], _S19->transform_0.data_0[int(1)][int(3)], _S19->transform_0.data_0[int(2)][int(3)], _S19->transform_0.data_0[int(3)][int(3)]), &kernelContext_6);

#line 1555
            overlay_0 = float4(_S21, 1.0f);

#line 1555
        }
        else
        {

#line 1555
            if(((&kernelContext_6)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1555
                overlay_0 = float4(lod_tint_0(((&kernelContext_6)->cluster_select_0[_S17].flags_1) >> 2U), 1.0f);

#line 1555
            }
            else
            {

#line 1555
                overlay_0 = _S20;

#line 1555
            }

#line 1555
        }

#line 1555
        ClusterSelect_0 _S22 = (&kernelContext_6)->cluster_select_0[_S17];

#line 1555
        bool _S23 = ((_S19->flags_0) & 2U) != 0U;

#line 1555
        uint base_vertex_2;

#line 1555
        if(_S23)
        {

#line 1555
            base_vertex_2 = _S19->base_vertex_0;

#line 1555
        }
        else
        {

#line 1555
            base_vertex_2 = mesh_1.base_vertex_1;

#line 1555
        }

#line 1555
        uint t_2;

#line 1555
        if(_S23)
        {

#line 1555
            t_2 = _S19->previous_base_vertex_0;

#line 1555
        }
        else
        {

#line 1555
            t_2 = base_vertex_2;

#line 1555
        }

#line 1555
        matrix<float,int(4),int(4)>  _S24 = matrix<float,int(4),int(4)> (_S19->transform_0.data_0[int(0)][int(0)], _S19->transform_0.data_0[int(1)][int(0)], _S19->transform_0.data_0[int(2)][int(0)], _S19->transform_0.data_0[int(3)][int(0)], _S19->transform_0.data_0[int(0)][int(1)], _S19->transform_0.data_0[int(1)][int(1)], _S19->transform_0.data_0[int(2)][int(1)], _S19->transform_0.data_0[int(3)][int(1)], _S19->transform_0.data_0[int(0)][int(2)], _S19->transform_0.data_0[int(1)][int(2)], _S19->transform_0.data_0[int(2)][int(2)], _S19->transform_0.data_0[int(3)][int(2)], _S19->transform_0.data_0[int(0)][int(3)], _S19->transform_0.data_0[int(1)][int(3)], _S19->transform_0.data_0[int(2)][int(3)], _S19->transform_0.data_0[int(3)][int(3)]);

#line 1555
        matrix<float,int(3),int(3)>  _S25 = normal_basis_0(matrix<float,int(3),int(3)> (_S24[int(0)].xyz, _S24[int(1)].xyz, _S24[int(2)].xyz));

#line 1555
        float4 _S26 = float4(mesh_1.uv_scale_u_0, mesh_1.uv_scale_v_0, mesh_1.uv_offset_u_0, mesh_1.uv_offset_v_0);

#line 1555
        uint v_1 = lane_2;

#line 1555
        for(;;)
        {

#line 1555
            if(v_1 < (cluster_1.vertex_count_0))
            {
            }
            else
            {

#line 1555
                break;
            }

#line 1555
            uint index_0 = (&kernelContext_6)->cluster_vertices_0[cluster_1.vertex_offset_1 + v_1];

#line 1555
            MeshVertex_0 _S27 = load_vertex_0(index_0 + base_vertex_2 + _S22.vertex_base_0, _S26, &kernelContext_6);

#line 1555
            float4 world_0 = (((float4(_S27.position_0, 1.0f)) * (_S24)));

#line 1555
            thread VertexOutput_0 output_0;

#line 1555
            (&output_0)->position_1 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_6)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 1555
            (&output_0)->world_position_0 = world_0.xyz;

#line 1555
            (&output_0)->world_normal_0 = (((_S27.basis_3.normal_0) * (_S25)));

#line 1555
            float4 _S28;

#line 1555
            if(((&kernelContext_6)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1555
                _S28 = overlay_0;

#line 1555
            }
            else
            {

#line 1555
                _S28 = _S27.color_0;

#line 1555
            }

#line 1555
            (&output_0)->color_1 = _S28;

#line 1555
            (&output_0)->material_1 = _S19->material_0;

#line 1555
            (&output_0)->uv_0 = _S27.uv0_0;

#line 1555
            float3 _S29 = load_position_0(index_0 + t_2 + _S22.vertex_base_0, &kernelContext_6);

#line 1555
            (&output_0)->clip_position_0 = (&output_0)->position_1;

#line 1555
            (&output_0)->previous_clip_position_0 = ((((((float4(_S29, 1.0f)) * (matrix<float,int(4),int(4)> (_S19->previous_transform_0.data_0[int(0)][int(0)], _S19->previous_transform_0.data_0[int(1)][int(0)], _S19->previous_transform_0.data_0[int(2)][int(0)], _S19->previous_transform_0.data_0[int(3)][int(0)], _S19->previous_transform_0.data_0[int(0)][int(1)], _S19->previous_transform_0.data_0[int(1)][int(1)], _S19->previous_transform_0.data_0[int(2)][int(1)], _S19->previous_transform_0.data_0[int(3)][int(1)], _S19->previous_transform_0.data_0[int(0)][int(2)], _S19->previous_transform_0.data_0[int(1)][int(2)], _S19->previous_transform_0.data_0[int(2)][int(2)], _S19->previous_transform_0.data_0[int(3)][int(2)], _S19->previous_transform_0.data_0[int(0)][int(3)], _S19->previous_transform_0.data_0[int(1)][int(3)], _S19->previous_transform_0.data_0[int(2)][int(3)], _S19->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_6)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));

#line 1555
            _slang_mesh.set_vertex(v_1,output_0);

#line 1555
            v_1 = v_1 + 64U;

#line 1555
        }

#line 1555
        t_2 = lane_2;

#line 1555
        for(;;)
        {

#line 1555
            if(t_2 < (cluster_1.triangle_count_0))
            {
            }
            else
            {

#line 1555
                break;
            }

#line 1555
            uint corner_1 = cluster_1.triangle_offset_0 + t_2 * 3U;

#line 1555
            uint _S30 = corner_at_0(corner_1, &kernelContext_6);

#line 1555
            uint _S31 = corner_at_0(corner_1 + 1U, &kernelContext_6);

#line 1555
            uint _S32 = corner_at_0(corner_1 + 2U, &kernelContext_6);

#line 1555
            _slang_mesh.set_index(t_2*3+0,(uint3(_S30, _S31, _S32))[0]);
            _slang_mesh.set_index(t_2*3+1,(uint3(_S30, _S31, _S32))[1]);
            _slang_mesh.set_index(t_2*3+2,(uint3(_S30, _S31, _S32))[2]);
            ;

#line 1555
            t_2 = t_2 + 64U;

#line 1555
        }

#line 1555
        break;
    }

#line 1556
    return;
}


#line 1358
uint cluster_is_selected_0(const ClusterSelect_0 thread* select_1, uint instance_index_0, KernelContext_0 thread* kernelContext_7)
{
    uint base_1 = instance_index_0 * kernelContext_7->draw_0->group_stride_0;

#line 1360
    uint _S33 = select_1->flags_1;

#line 1360
    bool producer_expanded_0;


    if(((select_1->flags_1) & 1U) != 0U)
    {

#line 1363
        producer_expanded_0 = kernelContext_7->group_state_0[base_1 + select_1->producer_group_0] != 0U;

#line 1363
    }
    else
    {

#line 1363
        producer_expanded_0 = false;

#line 1363
    }

#line 1363
    bool container_expanded_0;

    if((_S33 & 2U) == 0U)
    {

#line 1365
        container_expanded_0 = true;

#line 1365
    }
    else
    {

#line 1365
        container_expanded_0 = kernelContext_7->group_state_0[base_1 + select_1->container_group_0] != 0U;

#line 1365
    }

    if(!producer_expanded_0)
    {

#line 1367
        producer_expanded_0 = container_expanded_0;

#line 1367
    }
    else
    {

#line 1367
        producer_expanded_0 = false;

#line 1367
    }

#line 1367
    uint _S34;

#line 1367
    if(producer_expanded_0)
    {

#line 1367
        _S34 = 1U;

#line 1367
    }
    else
    {

#line 1367
        _S34 = 0U;

#line 1367
    }

#line 1367
    return _S34;
}


#line 1220
bool preserves_angles_0(matrix<float,int(3),int(3)>  basis_4)
{
    matrix<float,int(3),int(3)>  gram_0 = (((basis_4) * (transpose(basis_4))));
    float _S35 = max(gram_0[int(0)][int(0)], max(gram_0[int(1)][int(1)], gram_0[int(2)][int(2)]));
    if(_S35 <= 0.0f)
    {
        return false;
    }
    float slack_0 = 0.00009999999747379f * _S35;

#line 1228
    bool _S36;
    if((abs(gram_0[int(0)][int(1)])) <= slack_0)
    {

#line 1229
        _S36 = (abs(gram_0[int(0)][int(2)])) <= slack_0;

#line 1229
    }
    else
    {

#line 1229
        _S36 = false;

#line 1229
    }

#line 1229
    if(_S36)
    {

#line 1229
        _S36 = (abs(gram_0[int(1)][int(2)])) <= slack_0;

#line 1229
    }
    else
    {

#line 1229
        _S36 = false;

#line 1229
    }
    if(_S36)
    {

#line 1230
        _S36 = (_S35 - gram_0[int(0)][int(0)]) <= slack_0;

#line 1230
    }
    else
    {

#line 1230
        _S36 = false;

#line 1230
    }

#line 1230
    if(_S36)
    {

#line 1230
        _S36 = (_S35 - gram_0[int(1)][int(1)]) <= slack_0;

#line 1230
    }
    else
    {

#line 1230
        _S36 = false;

#line 1230
    }
    if(_S36)
    {

#line 1231
        _S36 = (_S35 - gram_0[int(2)][int(2)]) <= slack_0;

#line 1231
    }
    else
    {

#line 1231
        _S36 = false;

#line 1231
    }

#line 1229
    return _S36;
}


#line 1301
uint cluster_survives_0(const Meshlet_0 thread* cluster_2, matrix<float,int(4),int(4)>  transform_2, KernelContext_0 thread* kernelContext_8)
{
    matrix<float,int(3),int(3)>  _S37 = matrix<float,int(3),int(3)> (transform_2[int(0)].xyz, transform_2[int(1)].xyz, transform_2[int(2)].xyz);
    float3 center_1 = (((float4(cluster_2->center_x_0, cluster_2->center_y_0, cluster_2->center_z_0, 1.0f)) * (transform_2))).xyz;
    float radius_3 = cluster_2->radius_0 * max_stretch_0(_S37);

#line 1305
    uint plane_0 = 0U;

    for(;;)
    {

#line 1307
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 1307
            break;
        }

        float3 _S38 = kernelContext_8->cull_0->planes_0[plane_0].xyz;

#line 1310
        if((dot(_S38, center_1) + kernelContext_8->cull_0->planes_0[plane_0].w) < (- radius_3 * length(_S38)))
        {
            return 1U;
        }

#line 1307
        plane_0 = plane_0 + 1U;

#line 1307
    }

#line 1324
    float3 axis_0 = (((float3(cluster_2->cone_axis_x_0, cluster_2->cone_axis_y_0, cluster_2->cone_axis_z_0)) * (_S37)));

    float axis_length_0 = length(axis_0);

#line 1326
    float3 axis_1;
    if(axis_length_0 > 0.0f)
    {

#line 1327
        axis_1 = axis_0 / float3(axis_length_0) ;

#line 1327
    }
    else
    {

#line 1327
        axis_1 = float3(0.0f, 0.0f, 0.0f);

#line 1327
    }
    float3 to_center_0 = center_1 - kernelContext_8->frame_0->camera_position_0.xyz;

#line 1328
    float _S39 = cluster_2->cone_cutoff_0;
    float sine_0 = sqrt(max(0.0f, 1.0f - cluster_2->cone_cutoff_0 * cluster_2->cone_cutoff_0));

#line 1329
    bool _S40;
    if(preserves_angles_0(_S37))
    {

#line 1330
        _S40 = _S39 > 0.0f;

#line 1330
    }
    else
    {

#line 1330
        _S40 = false;

#line 1330
    }
    if(_S40)
    {

#line 1331
        _S40 = (dot(axis_1, to_center_0)) > (sine_0 * length(to_center_0) + radius_3);

#line 1331
    }
    else
    {

#line 1331
        _S40 = false;

#line 1331
    }

#line 1330
    if(_S40)
    {

        return 2U;
    }

    return 0U;
}


#line 1580
[[object]] void taskMain(uint3 group_3 [[threadgroup_position_in_grid]], ClusterPayload_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp, ClusterDrawConstants_0 constant* draw_2 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_2 [[buffer(10)]], Meshlet_0 device* clusters_2 [[buffer(7)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], ClusterSelect_0 device* cluster_select_2 [[buffer(13)]], uint device* tables_2 [[buffer(19)]], uint device* cluster_vertices_2 [[buffer(8)]], uint device* vertices_2 [[buffer(1)]], uint device* cluster_corners_2 [[buffer(9)]], uint device* group_state_2 [[buffer(15)]], CullParams_0 constant* cull_2 [[buffer(11)]], atomic<uint> device* cull_stats_2 [[buffer(12)]], uint device* cluster_selection_2 [[buffer(14)]])
{

#line 1580
    thread KernelContext_0 kernelContext_9;

#line 1580
    (&kernelContext_9)->draw_0 = draw_2;

#line 1580
    (&kernelContext_9)->draw_args_0 = draw_args_2;

#line 1580
    (&kernelContext_9)->clusters_0 = clusters_2;

#line 1580
    (&kernelContext_9)->visible_instances_0 = visible_instances_2;

#line 1580
    (&kernelContext_9)->instances_0 = instances_2;

#line 1580
    (&kernelContext_9)->meshes_0 = meshes_2;

#line 1580
    (&kernelContext_9)->frame_0 = frame_2;

#line 1580
    (&kernelContext_9)->cluster_select_0 = cluster_select_2;

#line 1580
    (&kernelContext_9)->tables_0 = tables_2;

#line 1580
    (&kernelContext_9)->cluster_vertices_0 = cluster_vertices_2;

#line 1580
    (&kernelContext_9)->vertices_0 = vertices_2;

#line 1580
    (&kernelContext_9)->cluster_corners_0 = cluster_corners_2;

#line 1580
    (&kernelContext_9)->group_state_0 = group_state_2;

#line 1580
    (&kernelContext_9)->cull_0 = cull_2;

#line 1580
    (&kernelContext_9)->cull_stats_0 = cull_stats_2;

#line 1580
    (&kernelContext_9)->cluster_selection_0 = cluster_selection_2;

#line 1580
    uint _S41 = group_is_live_0(group_3, &kernelContext_9);


    uint _S42 = group_3.x;

#line 1583
    uint _S43 = _S42 * _S41;

#line 1583
    Meshlet_0 cluster_3 = (&kernelContext_9)->clusters_0[(&kernelContext_9)->draw_0->cluster_base_0 + _S43];
    uint _S44 = group_3.y;

#line 1584
    uint instance_index_1 = (&kernelContext_9)->visible_instances_0[(&kernelContext_9)->draw_0->base_0 + _S44 * _S41] * _S41;

#line 1584
    GpuInstance_natural_0 device* _S45 = (&kernelContext_9)->instances_0+instance_index_1;


    uint index_1 = (&kernelContext_9)->draw_0->cluster_base_0 + _S43;

#line 1587
    thread ClusterSelect_0 _S46 = (&kernelContext_9)->cluster_select_0[index_1];

#line 1587
    uint _S47 = cluster_is_selected_0(&_S46, instance_index_1, &kernelContext_9);

#line 1587
    matrix<float,int(4),int(4)>  _S48 = matrix<float,int(4),int(4)> (_S45->transform_0.data_0[int(0)][int(0)], _S45->transform_0.data_0[int(1)][int(0)], _S45->transform_0.data_0[int(2)][int(0)], _S45->transform_0.data_0[int(3)][int(0)], _S45->transform_0.data_0[int(0)][int(1)], _S45->transform_0.data_0[int(1)][int(1)], _S45->transform_0.data_0[int(2)][int(1)], _S45->transform_0.data_0[int(3)][int(1)], _S45->transform_0.data_0[int(0)][int(2)], _S45->transform_0.data_0[int(1)][int(2)], _S45->transform_0.data_0[int(2)][int(2)], _S45->transform_0.data_0[int(3)][int(2)], _S45->transform_0.data_0[int(0)][int(3)], _S45->transform_0.data_0[int(1)][int(3)], _S45->transform_0.data_0[int(2)][int(3)], _S45->transform_0.data_0[int(3)][int(3)]);

#line 1587
    thread Meshlet_0 _S49 = cluster_3;

#line 1587
    uint _S50 = cluster_survives_0(&_S49, _S48, &kernelContext_9);


    uint _S51 = _S41 * _S47;

#line 1590
    bool _S52 = _S50 == 0U;

#line 1590
    uint word_4;

#line 1590
    if(_S52)
    {

#line 1590
        word_4 = 1U;

#line 1590
    }
    else
    {

#line 1590
        word_4 = 0U;

#line 1590
    }

#line 1590
    uint keep_0 = _S51 * word_4;

#line 1603
    if(_S52)
    {

#line 1603
        word_4 = 1U;

#line 1603
    }
    else
    {

#line 1604
        if(_S50 == 1U)
        {

#line 1604
            word_4 = 3U;

#line 1604
        }
        else
        {

#line 1604
            word_4 = 4U;

#line 1604
        }

#line 1603
    }


    if(_S51 == 1U)
    {
        uint _S53 = atomic_fetch_add_explicit((&kernelContext_9)->cull_stats_0+word_4, 1U, memory_order_relaxed);

#line 1606
    }

#line 1606
    bool _S54;

#line 1621
    if(_S44 == 0U)
    {

#line 1621
        _S54 = _S41 == 1U;

#line 1621
    }
    else
    {

#line 1621
        _S54 = false;

#line 1621
    }

#line 1621
    if(_S54)
    {
        *((&kernelContext_9)->cluster_selection_0+index_1) = _S47;

#line 1621
    }

#line 1626
    thread ClusterPayload_0 payload_0;
    (&payload_0)->cluster_0 = _S42;
    (&payload_0)->instance_0 = _S44;
    *_slang_mesh_payload = *(&payload_0); _slang_mgp.set_threadgroups_per_grid(uint3((keep_0), (1U), (1U))); return;;
    return;
}


#line 1641
[[mesh]] void amplifiedMeshMain(uint3 lane_3 [[thread_position_in_threadgroup]], const ClusterPayload_0 object_data* amplification_0 [[payload]], metal::mesh<VertexOutput_0, void, 64U, 124U, metal::topology::triangle> _slang_mesh, ClusterDrawConstants_0 constant* draw_3 [[buffer(3)]], DrawIndexedArgs_0 device* draw_args_3 [[buffer(10)]], Meshlet_0 device* clusters_3 [[buffer(7)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], ClusterSelect_0 device* cluster_select_3 [[buffer(13)]], uint device* tables_3 [[buffer(19)]], uint device* cluster_vertices_3 [[buffer(8)]], uint device* vertices_3 [[buffer(1)]], uint device* cluster_corners_3 [[buffer(9)]], uint device* group_state_3 [[buffer(15)]], CullParams_0 constant* cull_3 [[buffer(11)]], atomic<uint> device* cull_stats_3 [[buffer(12)]], uint device* cluster_selection_3 [[buffer(14)]])
{
    thread KernelContext_0 kernelContext_10;

#line 1643
    (&kernelContext_10)->draw_0 = draw_3;

#line 1643
    (&kernelContext_10)->draw_args_0 = draw_args_3;

#line 1643
    (&kernelContext_10)->clusters_0 = clusters_3;

#line 1643
    (&kernelContext_10)->visible_instances_0 = visible_instances_3;

#line 1643
    (&kernelContext_10)->instances_0 = instances_3;

#line 1643
    (&kernelContext_10)->meshes_0 = meshes_3;

#line 1643
    (&kernelContext_10)->frame_0 = frame_3;

#line 1643
    (&kernelContext_10)->cluster_select_0 = cluster_select_3;

#line 1643
    (&kernelContext_10)->tables_0 = tables_3;

#line 1643
    (&kernelContext_10)->cluster_vertices_0 = cluster_vertices_3;

#line 1643
    (&kernelContext_10)->vertices_0 = vertices_3;

#line 1643
    (&kernelContext_10)->cluster_corners_0 = cluster_corners_3;

#line 1643
    (&kernelContext_10)->group_state_0 = group_state_3;

#line 1643
    (&kernelContext_10)->cull_0 = cull_3;

#line 1643
    (&kernelContext_10)->cull_stats_0 = cull_stats_3;

#line 1643
    (&kernelContext_10)->cluster_selection_0 = cluster_selection_3;

#line 1643
    uint lane_4 = lane_3.x;

#line 1649
    uint _S55 = draw_3->cluster_base_0 + amplification_0->cluster_0;

#line 1647
    uint _S56 = amplification_0->instance_0;

#line 1647
    for(;;)
    {

#line 1647
        Meshlet_0 cluster_4 = (&kernelContext_10)->clusters_0[_S55];

#line 1647
        _slang_mesh.set_primitive_count((cluster_4.triangle_count_0));

#line 1647
        GpuInstance_natural_0 device* _S57 = (&kernelContext_10)->instances_0+(&kernelContext_10)->visible_instances_0[(&kernelContext_10)->draw_0->base_0 + _S56];

#line 1647
        GpuMesh_0 mesh_2 = (&kernelContext_10)->meshes_0[_S57->mesh_0];

#line 1647
        float4 _S58 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 1647
        float4 overlay_1;

#line 1647
        if(((&kernelContext_10)->frame_0->ambient_0.w) >= 2.5f)
        {

#line 1647
            float3 _S59 = cluster_heat_0(_S55, matrix<float,int(4),int(4)> (_S57->transform_0.data_0[int(0)][int(0)], _S57->transform_0.data_0[int(1)][int(0)], _S57->transform_0.data_0[int(2)][int(0)], _S57->transform_0.data_0[int(3)][int(0)], _S57->transform_0.data_0[int(0)][int(1)], _S57->transform_0.data_0[int(1)][int(1)], _S57->transform_0.data_0[int(2)][int(1)], _S57->transform_0.data_0[int(3)][int(1)], _S57->transform_0.data_0[int(0)][int(2)], _S57->transform_0.data_0[int(1)][int(2)], _S57->transform_0.data_0[int(2)][int(2)], _S57->transform_0.data_0[int(3)][int(2)], _S57->transform_0.data_0[int(0)][int(3)], _S57->transform_0.data_0[int(1)][int(3)], _S57->transform_0.data_0[int(2)][int(3)], _S57->transform_0.data_0[int(3)][int(3)]), &kernelContext_10);

#line 1647
            overlay_1 = float4(_S59, 1.0f);

#line 1647
        }
        else
        {

#line 1647
            if(((&kernelContext_10)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1647
                overlay_1 = float4(lod_tint_0(((&kernelContext_10)->cluster_select_0[_S55].flags_1) >> 2U), 1.0f);

#line 1647
            }
            else
            {

#line 1647
                overlay_1 = _S58;

#line 1647
            }

#line 1647
        }

#line 1647
        ClusterSelect_0 _S60 = (&kernelContext_10)->cluster_select_0[_S55];

#line 1647
        bool _S61 = ((_S57->flags_0) & 2U) != 0U;

#line 1647
        uint base_vertex_3;

#line 1647
        if(_S61)
        {

#line 1647
            base_vertex_3 = _S57->base_vertex_0;

#line 1647
        }
        else
        {

#line 1647
            base_vertex_3 = mesh_2.base_vertex_1;

#line 1647
        }

#line 1647
        uint t_3;

#line 1647
        if(_S61)
        {

#line 1647
            t_3 = _S57->previous_base_vertex_0;

#line 1647
        }
        else
        {

#line 1647
            t_3 = base_vertex_3;

#line 1647
        }

#line 1647
        matrix<float,int(4),int(4)>  _S62 = matrix<float,int(4),int(4)> (_S57->transform_0.data_0[int(0)][int(0)], _S57->transform_0.data_0[int(1)][int(0)], _S57->transform_0.data_0[int(2)][int(0)], _S57->transform_0.data_0[int(3)][int(0)], _S57->transform_0.data_0[int(0)][int(1)], _S57->transform_0.data_0[int(1)][int(1)], _S57->transform_0.data_0[int(2)][int(1)], _S57->transform_0.data_0[int(3)][int(1)], _S57->transform_0.data_0[int(0)][int(2)], _S57->transform_0.data_0[int(1)][int(2)], _S57->transform_0.data_0[int(2)][int(2)], _S57->transform_0.data_0[int(3)][int(2)], _S57->transform_0.data_0[int(0)][int(3)], _S57->transform_0.data_0[int(1)][int(3)], _S57->transform_0.data_0[int(2)][int(3)], _S57->transform_0.data_0[int(3)][int(3)]);

#line 1647
        matrix<float,int(3),int(3)>  _S63 = normal_basis_0(matrix<float,int(3),int(3)> (_S62[int(0)].xyz, _S62[int(1)].xyz, _S62[int(2)].xyz));

#line 1647
        float4 _S64 = float4(mesh_2.uv_scale_u_0, mesh_2.uv_scale_v_0, mesh_2.uv_offset_u_0, mesh_2.uv_offset_v_0);

#line 1647
        uint v_2 = lane_4;

#line 1647
        for(;;)
        {

#line 1647
            if(v_2 < (cluster_4.vertex_count_0))
            {
            }
            else
            {

#line 1647
                break;
            }

#line 1647
            uint index_2 = (&kernelContext_10)->cluster_vertices_0[cluster_4.vertex_offset_1 + v_2];

#line 1647
            MeshVertex_0 _S65 = load_vertex_0(index_2 + base_vertex_3 + _S60.vertex_base_0, _S64, &kernelContext_10);

#line 1647
            float4 world_1 = (((float4(_S65.position_0, 1.0f)) * (_S62)));

#line 1647
            thread VertexOutput_0 output_1;

#line 1647
            (&output_1)->position_1 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_10)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_10)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 1647
            (&output_1)->world_position_0 = world_1.xyz;

#line 1647
            (&output_1)->world_normal_0 = (((_S65.basis_3.normal_0) * (_S63)));

#line 1647
            float4 _S66;

#line 1647
            if(((&kernelContext_10)->frame_0->ambient_0.w) >= 1.5f)
            {

#line 1647
                _S66 = overlay_1;

#line 1647
            }
            else
            {

#line 1647
                _S66 = _S65.color_0;

#line 1647
            }

#line 1647
            (&output_1)->color_1 = _S66;

#line 1647
            (&output_1)->material_1 = _S57->material_0;

#line 1647
            (&output_1)->uv_0 = _S65.uv0_0;

#line 1647
            float3 _S67 = load_position_0(index_2 + t_3 + _S60.vertex_base_0, &kernelContext_10);

#line 1647
            (&output_1)->clip_position_0 = (&output_1)->position_1;

#line 1647
            (&output_1)->previous_clip_position_0 = ((((((float4(_S67, 1.0f)) * (matrix<float,int(4),int(4)> (_S57->previous_transform_0.data_0[int(0)][int(0)], _S57->previous_transform_0.data_0[int(1)][int(0)], _S57->previous_transform_0.data_0[int(2)][int(0)], _S57->previous_transform_0.data_0[int(3)][int(0)], _S57->previous_transform_0.data_0[int(0)][int(1)], _S57->previous_transform_0.data_0[int(1)][int(1)], _S57->previous_transform_0.data_0[int(2)][int(1)], _S57->previous_transform_0.data_0[int(3)][int(1)], _S57->previous_transform_0.data_0[int(0)][int(2)], _S57->previous_transform_0.data_0[int(1)][int(2)], _S57->previous_transform_0.data_0[int(2)][int(2)], _S57->previous_transform_0.data_0[int(3)][int(2)], _S57->previous_transform_0.data_0[int(0)][int(3)], _S57->previous_transform_0.data_0[int(1)][int(3)], _S57->previous_transform_0.data_0[int(2)][int(3)], _S57->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_10)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));

#line 1647
            _slang_mesh.set_vertex(v_2,output_1);

#line 1647
            v_2 = v_2 + 64U;

#line 1647
        }

#line 1647
        t_3 = lane_4;

#line 1647
        for(;;)
        {

#line 1647
            if(t_3 < (cluster_4.triangle_count_0))
            {
            }
            else
            {

#line 1647
                break;
            }

#line 1647
            uint corner_2 = cluster_4.triangle_offset_0 + t_3 * 3U;

#line 1647
            uint _S68 = corner_at_0(corner_2, &kernelContext_10);

#line 1647
            uint _S69 = corner_at_0(corner_2 + 1U, &kernelContext_10);

#line 1647
            uint _S70 = corner_at_0(corner_2 + 2U, &kernelContext_10);

#line 1647
            _slang_mesh.set_index(t_3*3+0,(uint3(_S68, _S69, _S70))[0]);
            _slang_mesh.set_index(t_3*3+1,(uint3(_S68, _S69, _S70))[1]);
            _slang_mesh.set_index(t_3*3+2,(uint3(_S68, _S69, _S70))[2]);
            ;

#line 1647
            t_3 = t_3 + 64U;

#line 1647
        }

#line 1647
        break;
    }

#line 1654
    return;
}

