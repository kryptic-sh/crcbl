#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 525 "shaders/draw_gen.slang"
struct DrawGenParams_0
{
    uint bucket_count_0;
    uint visible_capacity_0;
    uint group_stride_0;
    uint bucket_modes_at_0;
    uint bucket_clusters_at_0;
    uint mesh_levels_at_0;
    uint level_groups_at_0;
    uint level_meshes_at_0;
    float4 camera_position_0;
    float4 lod_params_0;
    uint mode_0;
    uint draw_regions_0;
    uint face_runs_at_0;
    uint mode_pad_0;
};


#line 309
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
    float uv_scale_u_0;
    float uv_scale_v_0;
    float uv_offset_u_0;
    float uv_offset_v_0;
    uint flags_0;
};


#line 1136
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1136
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_1;
    uint base_vertex_1;
    uint previous_base_vertex_0;
    uint pad1_0;
    uint pad2_0;
};


#line 1324
struct KernelContext_0
{
    DrawGenParams_0 constant* gen_0;
    uint device* tables_0;
    GpuMesh_0 device* meshes_0;
    uint device* visible_instances_0;
    atomic<uint> device* args_0;
    atomic<uint> device* counts_and_mesh_args_0;
    uint device* visible_count_0;
    GpuInstance_natural_0 device* instances_0;
    uint device* group_state_0;
    array<uint, int(256)> threadgroup* starts_chunk_runs_0;
};


#line 786
uint bucket_mesh_0(uint bucket_0, KernelContext_0 thread* kernelContext_0)
{
    return kernelContext_0->tables_0[bucket_0];
}


#line 891
uint draw_slot_0(uint region_0, uint bucket_1, KernelContext_0 thread* kernelContext_1)
{
    return region_0 * kernelContext_1->gen_0->bucket_count_0 + bucket_1;
}


#line 891
uint draw_slot_1(uint region_1, uint bucket_2, KernelContext_0 thread* kernelContext_2)
{
    return region_1 * kernelContext_2->gen_0->bucket_count_0 + bucket_2;
}


#line 902
uint run_start_word_0(uint region_2, uint bucket_3, KernelContext_0 thread* kernelContext_3)
{
    uint _S1 = 3U * kernelContext_3->gen_0->visible_capacity_0;

#line 904
    uint _S2 = draw_slot_0(region_2, bucket_3, kernelContext_3);

#line 904
    return _S1 + _S2;
}


#line 902
uint run_start_word_1(uint region_3, uint bucket_4, KernelContext_0 thread* kernelContext_4)
{
    uint _S3 = 3U * kernelContext_4->gen_0->visible_capacity_0;

#line 904
    uint _S4 = draw_slot_0(region_3, bucket_4, kernelContext_4);

#line 904
    return _S3 + _S4;
}


#line 916
uint bucket_mesh_word_0(uint bucket_5, KernelContext_0 thread* kernelContext_5)
{

#line 916
    uint _S5 = run_start_word_1(7U, bucket_5, kernelContext_5);

    return _S5;
}


#line 938
uint arg_word_0(uint region_4, uint bucket_6, uint field_0, KernelContext_0 thread* kernelContext_6)
{

#line 938
    uint _S6 = draw_slot_1(region_4, bucket_6, kernelContext_6);

    return _S6 * 5U + field_0;
}


#line 931
uint mesh_arg_word_0(uint region_5, uint bucket_7, uint slot_0, KernelContext_0 thread* kernelContext_7)
{
    return region_5 * 4U * kernelContext_7->gen_0->bucket_count_0 + kernelContext_7->gen_0->bucket_count_0 + bucket_7 * 3U + slot_0;
}


#line 819
uint bucket_clusters_0(uint bucket_8, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->tables_0[kernelContext_8->gen_0->bucket_clusters_at_0 + bucket_8];
}


#line 1102
uint survivor_count_0(KernelContext_0 thread* kernelContext_9)
{
    return min(kernelContext_9->visible_count_0[0U], kernelContext_9->gen_0->visible_capacity_0);
}


#line 490
struct MeshLevels_0
{
    uint first_group_0;
    uint group_count_0;
    uint first_level_0;
    uint top_level_0;
};


#line 826
MeshLevels_0 mesh_levels_of_0(uint mesh_1, KernelContext_0 thread* kernelContext_10)
{
    uint at_0 = kernelContext_10->gen_0->mesh_levels_at_0 + mesh_1 * 4U;
    thread MeshLevels_0 levels_0;
    (&levels_0)->first_group_0 = kernelContext_10->tables_0[at_0];
    (&levels_0)->group_count_0 = kernelContext_10->tables_0[at_0 + 1U];
    (&levels_0)->first_level_0 = kernelContext_10->tables_0[at_0 + 2U];
    (&levels_0)->top_level_0 = kernelContext_10->tables_0[at_0 + 3U];
    return levels_0;
}


#line 1009
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S7 = (((basis_0) * (transpose(basis_0))));

#line 1011
    float bound_0 = 0.0f;

#line 1011
    uint row_0 = 0U;

    for(;;)
    {

#line 1013
        if(row_0 < 3U)
        {
        }
        else
        {

#line 1013
            break;
        }
        float _S8 = max(bound_0, abs(_S7[row_0][int(0)]) + abs(_S7[row_0][int(1)]) + abs(_S7[row_0][int(2)]));

#line 1013
        uint row_1 = row_0 + 1U;

#line 1013
        bound_0 = _S8;

#line 1013
        row_0 = row_1;

#line 1013
    }



    return sqrt(bound_0);
}


#line 445
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_0;
    float center_y_0;
    float center_z_0;
    float radius_0;
};


#line 844
LevelGroup_0 level_group_at_0(uint group_0, KernelContext_0 thread* kernelContext_11)
{
    uint at_1 = kernelContext_11->gen_0->level_groups_at_0 + group_0 * 6U;
    thread LevelGroup_0 record_0;
    (&record_0)->level_0 = kernelContext_11->tables_0[at_1];
    (&record_0)->error_0 = (as_type<float>((kernelContext_11->tables_0[at_1 + 1U])));
    (&record_0)->center_x_0 = (as_type<float>((kernelContext_11->tables_0[at_1 + 2U])));
    (&record_0)->center_y_0 = (as_type<float>((kernelContext_11->tables_0[at_1 + 3U])));
    (&record_0)->center_z_0 = (as_type<float>((kernelContext_11->tables_0[at_1 + 4U])));
    (&record_0)->radius_0 = (as_type<float>((kernelContext_11->tables_0[at_1 + 5U])));
    return record_0;
}


#line 975
float projected_error_0(float error_1, float3 center_0, float radius_1, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S9 = delta_0.x;

#line 978
    float _S10 = delta_0.y;

#line 978
    float _S11 = delta_0.z;
    float distance_0 = sqrt(_S9 * _S9 + _S10 * _S10 + _S11 * _S11) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 1029
uint group_is_expanded_0(float error_2, float3 center_1, float radius_2, float3 eye_1, uint was_0, KernelContext_0 thread* kernelContext_12)
{
    float projected_0 = projected_error_0(error_2, center_1, radius_2, eye_1, kernelContext_12->gen_0->lod_params_0.x);

#line 1031
    bool expanded_0;

    if(projected_0 > (kernelContext_12->gen_0->lod_params_0.y))
    {

#line 1033
        expanded_0 = true;

#line 1033
    }
    else
    {

#line 1033
        if(was_0 != 0U)
        {

#line 1033
            expanded_0 = projected_0 > (kernelContext_12->gen_0->lod_params_0.z);

#line 1033
        }
        else
        {

#line 1033
            expanded_0 = false;

#line 1033
        }

#line 1033
    }

#line 1033
    uint _S12;
    if(expanded_0)
    {

#line 1034
        _S12 = 1U;

#line 1034
    }
    else
    {

#line 1034
        _S12 = 0U;

#line 1034
    }

#line 1034
    return _S12;
}


#line 863
uint level_mesh_at_0(uint level_1, KernelContext_0 thread* kernelContext_13)
{
    return kernelContext_13->tables_0[kernelContext_13->gen_0->level_meshes_at_0 + level_1];
}


#line 797
uint bucket_mode_0(uint bucket_9, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->tables_0[kernelContext_14->gen_0->bucket_modes_at_0 + bucket_9];
}


#line 875
uint route_word_0(uint survivor_0, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->gen_0->visible_capacity_0 + survivor_0;
}


#line 1117
uint select_level_0(uint _S13, uint _S14, KernelContext_0 thread* kernelContext_16)
{

#line 1117
    GpuInstance_natural_0 device* _S15 = kernelContext_16->instances_0+_S13;

#line 1117
    MeshLevels_0 _S16 = mesh_levels_of_0(_S15->mesh_0, kernelContext_16);

#line 1075
    float3 _S17 = kernelContext_16->gen_0->camera_position_0.xyz;

#line 1075
    matrix<float,int(4),int(4)>  _S18 = matrix<float,int(4),int(4)> (_S15->transform_0.data_0[int(0)][int(0)], _S15->transform_0.data_0[int(1)][int(0)], _S15->transform_0.data_0[int(2)][int(0)], _S15->transform_0.data_0[int(3)][int(0)], _S15->transform_0.data_0[int(0)][int(1)], _S15->transform_0.data_0[int(1)][int(1)], _S15->transform_0.data_0[int(2)][int(1)], _S15->transform_0.data_0[int(3)][int(1)], _S15->transform_0.data_0[int(0)][int(2)], _S15->transform_0.data_0[int(1)][int(2)], _S15->transform_0.data_0[int(2)][int(2)], _S15->transform_0.data_0[int(3)][int(2)], _S15->transform_0.data_0[int(0)][int(3)], _S15->transform_0.data_0[int(1)][int(3)], _S15->transform_0.data_0[int(2)][int(3)], _S15->transform_0.data_0[int(3)][int(3)]);
    float _S19 = max_stretch_0(matrix<float,int(3),int(3)> (_S18[int(0)].xyz, _S18[int(1)].xyz, _S18[int(2)].xyz));
    uint _S20 = _S14 * kernelContext_16->gen_0->group_stride_0;

#line 1077
    uint chosen_0 = _S16.top_level_0;

#line 1077
    uint i_0 = 0U;

    for(;;)
    {

#line 1079
        if(i_0 < (_S16.group_count_0))
        {
        }
        else
        {

#line 1079
            break;
        }
        uint at_2 = _S16.first_group_0 + i_0;

#line 1081
        LevelGroup_0 _S21 = level_group_at_0(at_2, kernelContext_16);

#line 1086
        uint _S22 = _S20 + at_2;

#line 1086
        uint _S23 = group_is_expanded_0(_S21.error_0 * _S19, (((float4(_S21.center_x_0, _S21.center_y_0, _S21.center_z_0, 1.0f)) * (_S18))).xyz, _S21.radius_0 * _S19, _S17, *(kernelContext_16->group_state_0+_S22), kernelContext_16);
        *(kernelContext_16->group_state_0+_S22) = _S23;

#line 1087
        bool _S24;
        if(_S23 == 1U)
        {

#line 1088
            _S24 = (_S21.level_0) < chosen_0;

#line 1088
        }
        else
        {

#line 1088
            _S24 = false;

#line 1088
        }

#line 1088
        if(_S24)
        {

#line 1088
            chosen_0 = _S21.level_0;

#line 1088
        }

#line 1079
        i_0 = i_0 + 1U;

#line 1079
    }

#line 1093
    return chosen_0;
}


#line 1093
uint instance_material_mode_0(uint _S25, KernelContext_0 thread* kernelContext_17)
{

#line 811
    return (((kernelContext_17->instances_0+_S25)->flags_1) & 12U) >> 2U;
}


#line 1117
[[kernel]] void binMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* tables_1 [[buffer(4)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], uint device* visible_instances_1 [[buffer(5)]], atomic<uint> device* args_1 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_1 [[buffer(7)]], uint device* visible_count_1 [[buffer(3)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], uint device* group_state_1 [[buffer(8)]])
{

#line 1117
    uint routed_0;

#line 1117
    thread KernelContext_0 kernelContext_18;

#line 1117
    (&kernelContext_18)->gen_0 = gen_1;

#line 1117
    (&kernelContext_18)->tables_0 = tables_1;

#line 1117
    (&kernelContext_18)->meshes_0 = meshes_1;

#line 1117
    (&kernelContext_18)->visible_instances_0 = visible_instances_1;

#line 1117
    (&kernelContext_18)->args_0 = args_1;

#line 1117
    (&kernelContext_18)->counts_and_mesh_args_0 = counts_and_mesh_args_1;

#line 1117
    (&kernelContext_18)->visible_count_0 = visible_count_1;

#line 1117
    (&kernelContext_18)->instances_0 = instances_1;

#line 1117
    (&kernelContext_18)->group_state_0 = group_state_1;

#line 1117
    threadgroup array<uint, int(256)> starts_chunk_runs_1;

#line 1117
    (&kernelContext_18)->starts_chunk_runs_0 = &starts_chunk_runs_1;

    uint index_0 = thread_0.x;

#line 1119
    uint bucket_10;

#line 1124
    if(index_0 < (gen_1->bucket_count_0))
    {

#line 1124
        uint _S26 = bucket_mesh_0(index_0, &kernelContext_18);

        GpuMesh_0 _S27 = (&kernelContext_18)->meshes_0[_S26];

#line 1126
        uint _S28 = bucket_mesh_word_0(index_0, &kernelContext_18);

#line 1131
        uint device* _S29 = (&kernelContext_18)->visible_instances_0+_S28;

#line 1131
        uint _S30 = bucket_mesh_0(index_0, &kernelContext_18);

#line 1131
        *_S29 = _S30;

#line 1131
        bucket_10 = 0U;


        for(;;)
        {

#line 1134
            if(bucket_10 < ((&kernelContext_18)->gen_0->draw_regions_0))
            {
            }
            else
            {

#line 1134
                break;
            }

#line 1134
            uint _S31 = arg_word_0(bucket_10, index_0, 0U, &kernelContext_18);

            atomic_store_explicit((&kernelContext_18)->args_0+_S31, _S27.index_count_0, memory_order_relaxed);

#line 1136
            uint _S32 = arg_word_0(bucket_10, index_0, 2U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->args_0+_S32, _S27.base_index_0, memory_order_relaxed);

#line 1137
            uint _S33 = arg_word_0(bucket_10, index_0, 3U, &kernelContext_18);

#line 1144
            atomic_store_explicit((&kernelContext_18)->args_0+_S33, 0U, memory_order_relaxed);

#line 1144
            uint _S34 = arg_word_0(bucket_10, index_0, 4U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->args_0+_S34, 0U, memory_order_relaxed);

#line 1145
            uint _S35 = mesh_arg_word_0(bucket_10, index_0, 0U, &kernelContext_18);

#line 1152
            atomic<uint> device* _S36 = (&kernelContext_18)->counts_and_mesh_args_0+_S35;

#line 1152
            uint _S37 = bucket_clusters_0(index_0, &kernelContext_18);

#line 1152
            atomic_store_explicit(_S36, _S37, memory_order_relaxed);

#line 1152
            uint _S38 = mesh_arg_word_0(bucket_10, index_0, 2U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S38, 1U, memory_order_relaxed);

#line 1134
            bucket_10 = bucket_10 + 1U;

#line 1134
        }

#line 1124
    }

#line 1124
    uint _S39 = survivor_count_0(&kernelContext_18);

#line 1159
    if(index_0 >= _S39)
    {
        return;
    }



    uint device* _S40 = (&kernelContext_18)->visible_instances_0+index_0;

#line 1166
    uint entry_0 = *_S40;


    uint instance_index_0 = (*_S40) & 16777215U;

#line 1169
    MeshLevels_0 _S41 = mesh_levels_of_0(((&kernelContext_18)->instances_0+instance_index_0)->mesh_0, &kernelContext_18);

#line 1169
    uint _S42 = select_level_0(instance_index_0, instance_index_0, &kernelContext_18);

#line 1169
    uint _S43 = level_mesh_at_0(_S41.first_level_0 + _S42, &kernelContext_18);

#line 1169
    uint _S44 = instance_material_mode_0(instance_index_0, &kernelContext_18);

#line 1169
    bucket_10 = 0U;

#line 1192
    for(;;)
    {

#line 1192
        if(bucket_10 < (gen_1->bucket_count_0))
        {
        }
        else
        {

#line 1192
            routed_0 = 4294967295U;

#line 1192
            break;
        }

#line 1192
        uint _S45 = bucket_mesh_0(bucket_10, &kernelContext_18);

#line 1192
        bool _S46;

        if(_S45 != _S43)
        {

#line 1194
            _S46 = true;

#line 1194
        }
        else
        {

#line 1194
            uint _S47 = bucket_mode_0(bucket_10, &kernelContext_18);

#line 1194
            _S46 = _S47 != _S44;

#line 1194
        }

#line 1194
        if(_S46)
        {
            bucket_10 = bucket_10 + 1U;

#line 1192
            continue;
        }

#line 1205
        if(((&kernelContext_18)->gen_0->mode_0) == 2U)
        {

#line 1205
            routed_0 = 0U;


            for(;;)
            {

#line 1208
                if(routed_0 < 6U)
                {
                }
                else
                {

#line 1208
                    break;
                }
                if(((entry_0 >> (24U + routed_0)) & 1U) != 0U)
                {

#line 1210
                    uint _S48 = mesh_arg_word_0(1U + routed_0, bucket_10, 1U, &kernelContext_18);


                    uint _S49 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S48, 1U, memory_order_relaxed);

#line 1210
                }

#line 1208
                routed_0 = routed_0 + 1U;

#line 1208
            }

#line 1205
        }
        else
        {

#line 1205
            uint _S50 = mesh_arg_word_0(0U, bucket_10, 1U, &kernelContext_18);

#line 1222
            uint _S51 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S50, 1U, memory_order_relaxed);
            if(((&kernelContext_18)->gen_0->mode_0) == 1U)
            {

#line 1223
                _S46 = (entry_0 & 2147483648U) == 0U;

#line 1223
            }
            else
            {

#line 1223
                _S46 = false;

#line 1223
            }

#line 1223
            if(_S46)
            {

#line 1223
                uint _S52 = mesh_arg_word_0(1U, bucket_10, 1U, &kernelContext_18);


                uint _S53 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S52, 1U, memory_order_relaxed);

#line 1223
            }

#line 1205
        }

#line 1205
        routed_0 = bucket_10;

#line 1229
        break;
    }

#line 1229
    uint _S54 = route_word_0(index_0, &kernelContext_18);

#line 1234
    *((&kernelContext_18)->visible_instances_0+_S54) = routed_0;
    return;
}


#line 1259
uint starts_slot_count_0(KernelContext_0 thread* kernelContext_19)
{

#line 1259
    uint _S55;

    if((kernelContext_19->gen_0->mode_0) == 2U)
    {

#line 1261
        _S55 = 6U * kernelContext_19->gen_0->bucket_count_0;

#line 1261
    }
    else
    {

#line 1261
        _S55 = kernelContext_19->gen_0->bucket_count_0;

#line 1261
    }

#line 1261
    return _S55;
}


uint starts_slot_region_0(uint slot_1, KernelContext_0 thread* kernelContext_20)
{

#line 1265
    uint _S56;

    if((kernelContext_20->gen_0->mode_0) == 2U)
    {

#line 1267
        uint _S57 = slot_1 / kernelContext_20->gen_0->bucket_count_0;

#line 1267
        _S56 = 1U + _S57;

#line 1267
    }
    else
    {

#line 1267
        _S56 = 0U;

#line 1267
    }

#line 1267
    return _S56;
}


uint starts_slot_bucket_0(uint slot_2, KernelContext_0 thread* kernelContext_21)
{

#line 1271
    uint _S58;

    if((kernelContext_21->gen_0->mode_0) == 2U)
    {

#line 1273
        uint _S59 = slot_2 % kernelContext_21->gen_0->bucket_count_0;

#line 1273
        _S58 = _S59;

#line 1273
    }
    else
    {

#line 1273
        _S58 = slot_2;

#line 1273
    }

#line 1273
    return _S58;
}



uint starts_slot_runs_0(uint slot_3, KernelContext_0 thread* kernelContext_22)
{

#line 1278
    uint _S60 = starts_slot_region_0(slot_3, kernelContext_22);

#line 1278
    uint _S61 = starts_slot_bucket_0(slot_3, kernelContext_22);

#line 1278
    uint _S62 = mesh_arg_word_0(_S60, _S61, 1U, kernelContext_22);


    uint _S63 = atomic_load_explicit(kernelContext_22->counts_and_mesh_args_0+_S62, memory_order_relaxed);

#line 1280
    return _S63;
}


#line 882
uint runs_at_0(KernelContext_0 thread* kernelContext_23)
{
    return 2U * kernelContext_23->gen_0->visible_capacity_0;
}


#line 924
uint count_word_0(uint region_6, uint bucket_11, KernelContext_0 thread* kernelContext_24)
{
    return region_6 * 4U * kernelContext_24->gen_0->bucket_count_0 + bucket_11;
}


#line 1307
[[kernel]] void startsMain(uint3 thread_1 [[thread_position_in_threadgroup]], DrawGenParams_0 constant* gen_2 [[buffer(0)]], uint device* tables_2 [[buffer(4)]], GpuMesh_0 device* meshes_2 [[buffer(2)]], uint device* visible_instances_2 [[buffer(5)]], atomic<uint> device* args_2 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_2 [[buffer(7)]], uint device* visible_count_2 [[buffer(3)]], GpuInstance_natural_0 device* instances_2 [[buffer(1)]], uint device* group_state_2 [[buffer(8)]])
{

#line 1307
    uint behind_0;

#line 1307
    thread KernelContext_0 kernelContext_25;

#line 1307
    (&kernelContext_25)->gen_0 = gen_2;

#line 1307
    (&kernelContext_25)->tables_0 = tables_2;

#line 1307
    (&kernelContext_25)->meshes_0 = meshes_2;

#line 1307
    (&kernelContext_25)->visible_instances_0 = visible_instances_2;

#line 1307
    (&kernelContext_25)->args_0 = args_2;

#line 1307
    (&kernelContext_25)->counts_and_mesh_args_0 = counts_and_mesh_args_2;

#line 1307
    (&kernelContext_25)->visible_count_0 = visible_count_2;

#line 1307
    (&kernelContext_25)->instances_0 = instances_2;

#line 1307
    (&kernelContext_25)->group_state_0 = group_state_2;

#line 1307
    threadgroup array<uint, int(256)> starts_chunk_runs_2;

#line 1307
    (&kernelContext_25)->starts_chunk_runs_0 = &starts_chunk_runs_2;

    uint lane_0 = thread_1.x;

#line 1309
    uint _S64 = starts_slot_count_0(&kernelContext_25);

    uint chunk_0 = (_S64 + 256U - 1U) / 256U;
    uint _S65 = min(lane_0 * chunk_0, _S64);
    uint _S66 = min(_S65 + chunk_0, _S64);

#line 1313
    uint slot_4 = _S65;

#line 1313
    uint total_0 = 0U;


    for(;;)
    {

#line 1316
        if(slot_4 < _S66)
        {
        }
        else
        {

#line 1316
            break;
        }

#line 1316
        uint _S67 = starts_slot_runs_0(slot_4, &kernelContext_25);

        uint total_1 = total_0 + _S67;

#line 1316
        slot_4 = slot_4 + 1U;

#line 1316
        total_0 = total_1;

#line 1316
    }

#line 1324
    (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0] = total_0;
    threadgroup_barrier(mem_flags::mem_threadgroup);

#line 1325
    uint reach_0 = 1U;
    for(;;)
    {

#line 1326
        if(reach_0 < 256U)
        {
        }
        else
        {

#line 1326
            break;
        }
        if(lane_0 >= reach_0)
        {

#line 1328
            behind_0 = (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0 - reach_0];

#line 1328
        }
        else
        {

#line 1328
            behind_0 = 0U;

#line 1328
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
        (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0] = (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0] + behind_0;
        threadgroup_barrier(mem_flags::mem_threadgroup);

#line 1326
        reach_0 = reach_0 << 1U;

#line 1326
    }

#line 1337
    if(((&kernelContext_25)->gen_0->mode_0) == 2U)
    {

#line 1337
        slot_4 = (&kernelContext_25)->gen_0->face_runs_at_0;

#line 1337
    }
    else
    {

#line 1337
        uint _S68 = runs_at_0(&kernelContext_25);

#line 1337
        slot_4 = _S68;

#line 1337
    }
    uint _S69 = slot_4 + (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0] - total_0;

#line 1338
    slot_4 = _S65;

#line 1338
    behind_0 = _S69;
    for(;;)
    {

#line 1339
        if(slot_4 < _S66)
        {
        }
        else
        {

#line 1339
            break;
        }

#line 1339
        uint _S70 = starts_slot_region_0(slot_4, &kernelContext_25);

#line 1339
        uint _S71 = starts_slot_bucket_0(slot_4, &kernelContext_25);

#line 1339
        uint _S72 = starts_slot_runs_0(slot_4, &kernelContext_25);

#line 1339
        uint _S73 = run_start_word_0(_S70, _S71, &kernelContext_25);

#line 1344
        *((&kernelContext_25)->visible_instances_0+_S73) = behind_0;


        if(_S72 != 0U)
        {

#line 1347
            uint _S74 = count_word_0(_S70, _S71, &kernelContext_25);

            atomic_store_explicit((&kernelContext_25)->counts_and_mesh_args_0+_S74, 1U, memory_order_relaxed);

#line 1347
        }



        if(((&kernelContext_25)->gen_0->mode_0) == 1U)
        {

#line 1351
            uint _S75 = mesh_arg_word_0(1U, _S71, 1U, &kernelContext_25);

#line 1357
            uint early_0 = atomic_load_explicit((&kernelContext_25)->counts_and_mesh_args_0+_S75, memory_order_relaxed);

#line 1357
            uint _S76 = run_start_word_0(1U, _S71, &kernelContext_25);
            *((&kernelContext_25)->visible_instances_0+_S76) = behind_0;

#line 1358
            uint _S77 = run_start_word_0(2U, _S71, &kernelContext_25);
            *((&kernelContext_25)->visible_instances_0+_S77) = behind_0 + early_0;
            if(early_0 != 0U)
            {

#line 1360
                uint _S78 = count_word_0(1U, _S71, &kernelContext_25);

                atomic_store_explicit((&kernelContext_25)->counts_and_mesh_args_0+_S78, 1U, memory_order_relaxed);

#line 1360
            }

#line 1351
        }

#line 1365
        uint start_0 = behind_0 + _S72;

#line 1339
        slot_4 = slot_4 + 1U;

#line 1339
        behind_0 = start_0;

#line 1339
    }

#line 1367
    return;
}


#line 1378
[[kernel]] void scatterMain(uint3 thread_2 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_3 [[buffer(0)]], uint device* tables_3 [[buffer(4)]], GpuMesh_0 device* meshes_3 [[buffer(2)]], uint device* visible_instances_3 [[buffer(5)]], atomic<uint> device* args_3 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_3 [[buffer(7)]], uint device* visible_count_3 [[buffer(3)]], GpuInstance_natural_0 device* instances_3 [[buffer(1)]], uint device* group_state_3 [[buffer(8)]])
{

#line 1378
    thread KernelContext_0 kernelContext_26;

#line 1378
    (&kernelContext_26)->gen_0 = gen_3;

#line 1378
    (&kernelContext_26)->tables_0 = tables_3;

#line 1378
    (&kernelContext_26)->meshes_0 = meshes_3;

#line 1378
    (&kernelContext_26)->visible_instances_0 = visible_instances_3;

#line 1378
    (&kernelContext_26)->args_0 = args_3;

#line 1378
    (&kernelContext_26)->counts_and_mesh_args_0 = counts_and_mesh_args_3;

#line 1378
    (&kernelContext_26)->visible_count_0 = visible_count_3;

#line 1378
    (&kernelContext_26)->instances_0 = instances_3;

#line 1378
    (&kernelContext_26)->group_state_0 = group_state_3;

#line 1378
    threadgroup array<uint, int(256)> starts_chunk_runs_3;

#line 1378
    (&kernelContext_26)->starts_chunk_runs_0 = &starts_chunk_runs_3;

    uint index_1 = thread_2.x;

#line 1380
    uint _S79 = survivor_count_0(&kernelContext_26);
    if(index_1 >= _S79)
    {
        return;
    }

#line 1383
    uint _S80 = route_word_0(index_1, &kernelContext_26);

    uint device* _S81 = (&kernelContext_26)->visible_instances_0+_S80;

#line 1385
    uint bucket_12 = *_S81;
    if((*_S81) == 4294967295U)
    {
        return;
    }
    uint device* _S82 = (&kernelContext_26)->visible_instances_0+index_1;

#line 1390
    uint entry_1 = *_S82;
    uint instance_index_1 = (*_S82) & 16777215U;

#line 1391
    uint face_0;
    if(((&kernelContext_26)->gen_0->mode_0) == 2U)
    {

#line 1392
        face_0 = 0U;

        for(;;)
        {

#line 1394
            if(face_0 < 6U)
            {
            }
            else
            {

#line 1394
                break;
            }
            if(((entry_1 >> (24U + face_0)) & 1U) == 0U)
            {
                face_0 = face_0 + 1U;

#line 1394
                continue;
            }

#line 1400
            uint region_7 = 1U + face_0;

#line 1400
            uint _S83 = arg_word_0(region_7, bucket_12, 1U, &kernelContext_26);
            uint face_slot_0 = atomic_fetch_add_explicit((&kernelContext_26)->args_0+_S83, 1U, memory_order_relaxed);

#line 1401
            uint _S84 = run_start_word_0(region_7, bucket_12, &kernelContext_26);
            *((&kernelContext_26)->visible_instances_0+(*((&kernelContext_26)->visible_instances_0+_S84) + face_slot_0)) = instance_index_1;

#line 1394
            face_0 = face_0 + 1U;

#line 1394
        }

#line 1405
        return;
    }

#line 1405
    bool _S85;


    if(((&kernelContext_26)->gen_0->mode_0) == 1U)
    {

#line 1408
        _S85 = (entry_1 & 2147483648U) != 0U;

#line 1408
    }
    else
    {

#line 1408
        _S85 = false;

#line 1408
    }

#line 1408
    if(_S85)
    {
        return;
    }
    if(((&kernelContext_26)->gen_0->mode_0) == 1U)
    {

#line 1412
        face_0 = 1U;

#line 1412
    }
    else
    {

#line 1412
        face_0 = 0U;

#line 1412
    }

#line 1412
    uint _S86 = arg_word_0(face_0, bucket_12, 1U, &kernelContext_26);
    uint slot_5 = atomic_fetch_add_explicit((&kernelContext_26)->args_0+_S86, 1U, memory_order_relaxed);

#line 1413
    uint _S87 = run_start_word_0(face_0, bucket_12, &kernelContext_26);
    *((&kernelContext_26)->visible_instances_0+(*((&kernelContext_26)->visible_instances_0+_S87) + slot_5)) = instance_index_1;
    return;
}


#line 1426
[[kernel]] void lateScatterMain(uint3 thread_3 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_4 [[buffer(0)]], uint device* tables_4 [[buffer(4)]], GpuMesh_0 device* meshes_4 [[buffer(2)]], uint device* visible_instances_4 [[buffer(5)]], atomic<uint> device* args_4 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_4 [[buffer(7)]], uint device* visible_count_4 [[buffer(3)]], GpuInstance_natural_0 device* instances_4 [[buffer(1)]], uint device* group_state_4 [[buffer(8)]])
{

#line 1426
    thread KernelContext_0 kernelContext_27;

#line 1426
    (&kernelContext_27)->gen_0 = gen_4;

#line 1426
    (&kernelContext_27)->tables_0 = tables_4;

#line 1426
    (&kernelContext_27)->meshes_0 = meshes_4;

#line 1426
    (&kernelContext_27)->visible_instances_0 = visible_instances_4;

#line 1426
    (&kernelContext_27)->args_0 = args_4;

#line 1426
    (&kernelContext_27)->counts_and_mesh_args_0 = counts_and_mesh_args_4;

#line 1426
    (&kernelContext_27)->visible_count_0 = visible_count_4;

#line 1426
    (&kernelContext_27)->instances_0 = instances_4;

#line 1426
    (&kernelContext_27)->group_state_0 = group_state_4;

#line 1426
    threadgroup array<uint, int(256)> starts_chunk_runs_4;

#line 1426
    (&kernelContext_27)->starts_chunk_runs_0 = &starts_chunk_runs_4;

    uint index_2 = thread_3.x;

#line 1428
    uint _S88 = survivor_count_0(&kernelContext_27);
    if(index_2 >= _S88)
    {
        return;
    }
    uint device* _S89 = (&kernelContext_27)->visible_instances_0+index_2;

#line 1433
    uint entry_2 = *_S89;
    if(((*_S89) & 1073741824U) == 0U)
    {
        return;
    }

#line 1436
    uint _S90 = route_word_0(index_2, &kernelContext_27);

    uint device* _S91 = (&kernelContext_27)->visible_instances_0+_S90;

#line 1438
    uint bucket_13 = *_S91;
    if((*_S91) == 4294967295U)
    {
        return;
    }

#line 1441
    uint _S92 = arg_word_0(2U, bucket_13, 1U, &kernelContext_27);

    uint slot_6 = atomic_fetch_add_explicit((&kernelContext_27)->args_0+_S92, 1U, memory_order_relaxed);

#line 1443
    uint _S93 = run_start_word_0(2U, bucket_13, &kernelContext_27);
    *((&kernelContext_27)->visible_instances_0+(*((&kernelContext_27)->visible_instances_0+_S93) + slot_6)) = entry_2 & 16777215U;

    return;
}


#line 1457
[[kernel]] void lateFinishMain(uint3 thread_4 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_5 [[buffer(0)]], uint device* tables_5 [[buffer(4)]], GpuMesh_0 device* meshes_5 [[buffer(2)]], uint device* visible_instances_5 [[buffer(5)]], atomic<uint> device* args_5 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_5 [[buffer(7)]], uint device* visible_count_5 [[buffer(3)]], GpuInstance_natural_0 device* instances_5 [[buffer(1)]], uint device* group_state_5 [[buffer(8)]])
{

#line 1457
    thread KernelContext_0 kernelContext_28;

#line 1457
    (&kernelContext_28)->gen_0 = gen_5;

#line 1457
    (&kernelContext_28)->tables_0 = tables_5;

#line 1457
    (&kernelContext_28)->meshes_0 = meshes_5;

#line 1457
    (&kernelContext_28)->visible_instances_0 = visible_instances_5;

#line 1457
    (&kernelContext_28)->args_0 = args_5;

#line 1457
    (&kernelContext_28)->counts_and_mesh_args_0 = counts_and_mesh_args_5;

#line 1457
    (&kernelContext_28)->visible_count_0 = visible_count_5;

#line 1457
    (&kernelContext_28)->instances_0 = instances_5;

#line 1457
    (&kernelContext_28)->group_state_0 = group_state_5;

#line 1457
    threadgroup array<uint, int(256)> starts_chunk_runs_5;

#line 1457
    (&kernelContext_28)->starts_chunk_runs_0 = &starts_chunk_runs_5;

    uint bucket_14 = thread_4.x;
    if(bucket_14 >= (gen_5->bucket_count_0))
    {
        return;
    }

#line 1462
    uint _S94 = arg_word_0(1U, bucket_14, 1U, &kernelContext_28);

    uint early_1 = atomic_load_explicit((&kernelContext_28)->args_0+_S94, memory_order_relaxed);

#line 1464
    uint _S95 = arg_word_0(2U, bucket_14, 1U, &kernelContext_28);
    uint late_0 = atomic_load_explicit((&kernelContext_28)->args_0+_S95, memory_order_relaxed);

#line 1465
    uint _S96 = mesh_arg_word_0(2U, bucket_14, 1U, &kernelContext_28);
    atomic_store_explicit((&kernelContext_28)->counts_and_mesh_args_0+_S96, late_0, memory_order_relaxed);
    if(late_0 != 0U)
    {

#line 1467
        uint _S97 = count_word_0(2U, bucket_14, &kernelContext_28);

        atomic_store_explicit((&kernelContext_28)->counts_and_mesh_args_0+_S97, 1U, memory_order_relaxed);

#line 1467
    }



    uint drawn_0 = early_1 + late_0;

#line 1471
    uint _S98 = arg_word_0(0U, bucket_14, 1U, &kernelContext_28);
    atomic_store_explicit((&kernelContext_28)->args_0+_S98, drawn_0, memory_order_relaxed);

#line 1472
    uint _S99 = mesh_arg_word_0(0U, bucket_14, 1U, &kernelContext_28);
    atomic_store_explicit((&kernelContext_28)->counts_and_mesh_args_0+_S99, drawn_0, memory_order_relaxed);

#line 1473
    uint _S100 = count_word_0(0U, bucket_14, &kernelContext_28);
    atomic<uint> device* _S101 = (&kernelContext_28)->counts_and_mesh_args_0+_S100;

#line 1474
    int _S102;

#line 1474
    if(drawn_0 != 0U)
    {

#line 1474
        _S102 = int(1);

#line 1474
    }
    else
    {

#line 1474
        _S102 = int(0);

#line 1474
    }

#line 1474
    atomic_store_explicit(_S101, uint(_S102), memory_order_relaxed);
    return;
}

