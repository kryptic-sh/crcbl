#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 533 "shaders/draw_gen.slang"
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
    uint bucket_lookup_at_0;
    uint task_lanes_0;
};


#line 317
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


#line 1177
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1177
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


#line 1391
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


#line 809
uint bucket_mesh_0(uint bucket_0, KernelContext_0 thread* kernelContext_0)
{
    return kernelContext_0->tables_0[bucket_0];
}


#line 932
uint draw_slot_0(uint region_0, uint bucket_1, KernelContext_0 thread* kernelContext_1)
{
    return region_0 * kernelContext_1->gen_0->bucket_count_0 + bucket_1;
}


#line 932
uint draw_slot_1(uint region_1, uint bucket_2, KernelContext_0 thread* kernelContext_2)
{
    return region_1 * kernelContext_2->gen_0->bucket_count_0 + bucket_2;
}


#line 943
uint run_start_word_0(uint region_2, uint bucket_3, KernelContext_0 thread* kernelContext_3)
{
    uint _S1 = 3U * kernelContext_3->gen_0->visible_capacity_0;

#line 945
    uint _S2 = draw_slot_0(region_2, bucket_3, kernelContext_3);

#line 945
    return _S1 + _S2;
}


#line 943
uint run_start_word_1(uint region_3, uint bucket_4, KernelContext_0 thread* kernelContext_4)
{
    uint _S3 = 3U * kernelContext_4->gen_0->visible_capacity_0;

#line 945
    uint _S4 = draw_slot_0(region_3, bucket_4, kernelContext_4);

#line 945
    return _S3 + _S4;
}


#line 957
uint bucket_mesh_word_0(uint bucket_5, KernelContext_0 thread* kernelContext_5)
{

#line 957
    uint _S5 = run_start_word_1(7U, bucket_5, kernelContext_5);

    return _S5;
}


#line 979
uint arg_word_0(uint region_4, uint bucket_6, uint field_0, KernelContext_0 thread* kernelContext_6)
{

#line 979
    uint _S6 = draw_slot_1(region_4, bucket_6, kernelContext_6);

    return _S6 * 5U + field_0;
}


#line 972
uint mesh_arg_word_0(uint region_5, uint bucket_7, uint slot_0, KernelContext_0 thread* kernelContext_7)
{
    return region_5 * 4U * kernelContext_7->gen_0->bucket_count_0 + kernelContext_7->gen_0->bucket_count_0 + bucket_7 * 3U + slot_0;
}


#line 832
uint bucket_clusters_0(uint bucket_8, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->tables_0[kernelContext_8->gen_0->bucket_clusters_at_0 + bucket_8];
}


#line 1143
uint survivor_count_0(KernelContext_0 thread* kernelContext_9)
{
    return min(kernelContext_9->visible_count_0[0U], kernelContext_9->gen_0->visible_capacity_0);
}


#line 498
struct MeshLevels_0
{
    uint first_group_0;
    uint group_count_0;
    uint first_level_0;
    uint top_level_0;
};


#line 839
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


#line 1050
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S7 = (((basis_0) * (transpose(basis_0))));

#line 1052
    float bound_0 = 0.0f;

#line 1052
    uint row_0 = 0U;

    for(;;)
    {

#line 1054
        if(row_0 < 3U)
        {
        }
        else
        {

#line 1054
            break;
        }
        float _S8 = max(bound_0, abs(_S7[row_0][int(0)]) + abs(_S7[row_0][int(1)]) + abs(_S7[row_0][int(2)]));

#line 1054
        uint row_1 = row_0 + 1U;

#line 1054
        bound_0 = _S8;

#line 1054
        row_0 = row_1;

#line 1054
    }



    return sqrt(bound_0);
}


#line 453
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_0;
    float center_y_0;
    float center_z_0;
    float radius_0;
};


#line 857
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


#line 1016
float projected_error_0(float error_1, float3 center_0, float radius_1, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S9 = delta_0.x;

#line 1019
    float _S10 = delta_0.y;

#line 1019
    float _S11 = delta_0.z;
    float distance_0 = sqrt(_S9 * _S9 + _S10 * _S10 + _S11 * _S11) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 1070
uint group_is_expanded_0(float error_2, float3 center_1, float radius_2, float3 eye_1, uint was_0, KernelContext_0 thread* kernelContext_12)
{
    float projected_0 = projected_error_0(error_2, center_1, radius_2, eye_1, kernelContext_12->gen_0->lod_params_0.x);

#line 1072
    bool expanded_0;

    if(projected_0 > (kernelContext_12->gen_0->lod_params_0.y))
    {

#line 1074
        expanded_0 = true;

#line 1074
    }
    else
    {

#line 1074
        if(was_0 != 0U)
        {

#line 1074
            expanded_0 = projected_0 > (kernelContext_12->gen_0->lod_params_0.z);

#line 1074
        }
        else
        {

#line 1074
            expanded_0 = false;

#line 1074
        }

#line 1074
    }

#line 1074
    uint _S12;
    if(expanded_0)
    {

#line 1075
        _S12 = 1U;

#line 1075
    }
    else
    {

#line 1075
        _S12 = 0U;

#line 1075
    }

#line 1075
    return _S12;
}


#line 876
uint level_mesh_at_0(uint level_1, KernelContext_0 thread* kernelContext_13)
{
    return kernelContext_13->tables_0[kernelContext_13->gen_0->level_meshes_at_0 + level_1];
}


#line 905
uint bucket_for_0(uint mesh_2, uint mode_1, KernelContext_0 thread* kernelContext_14)
{
    if(mesh_2 >= (kernelContext_14->tables_0)[kernelContext_14->gen_0->bucket_lookup_at_0])
    {
        return 4294967295U;
    }
    return kernelContext_14->tables_0[kernelContext_14->gen_0->bucket_lookup_at_0 + 1U + mesh_2 * 4U + mode_1];
}



uint route_word_0(uint survivor_0, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->gen_0->visible_capacity_0 + survivor_0;
}


#line 1158
uint select_level_0(uint _S13, uint _S14, KernelContext_0 thread* kernelContext_16)
{

#line 1158
    GpuInstance_natural_0 device* _S15 = kernelContext_16->instances_0+_S13;

#line 1158
    MeshLevels_0 _S16 = mesh_levels_of_0(_S15->mesh_0, kernelContext_16);

#line 1116
    float3 _S17 = kernelContext_16->gen_0->camera_position_0.xyz;

#line 1116
    matrix<float,int(4),int(4)>  _S18 = matrix<float,int(4),int(4)> (_S15->transform_0.data_0[int(0)][int(0)], _S15->transform_0.data_0[int(1)][int(0)], _S15->transform_0.data_0[int(2)][int(0)], _S15->transform_0.data_0[int(3)][int(0)], _S15->transform_0.data_0[int(0)][int(1)], _S15->transform_0.data_0[int(1)][int(1)], _S15->transform_0.data_0[int(2)][int(1)], _S15->transform_0.data_0[int(3)][int(1)], _S15->transform_0.data_0[int(0)][int(2)], _S15->transform_0.data_0[int(1)][int(2)], _S15->transform_0.data_0[int(2)][int(2)], _S15->transform_0.data_0[int(3)][int(2)], _S15->transform_0.data_0[int(0)][int(3)], _S15->transform_0.data_0[int(1)][int(3)], _S15->transform_0.data_0[int(2)][int(3)], _S15->transform_0.data_0[int(3)][int(3)]);
    float _S19 = max_stretch_0(matrix<float,int(3),int(3)> (_S18[int(0)].xyz, _S18[int(1)].xyz, _S18[int(2)].xyz));
    uint _S20 = _S14 * kernelContext_16->gen_0->group_stride_0;

#line 1118
    uint chosen_0 = _S16.top_level_0;

#line 1118
    uint i_0 = 0U;

    for(;;)
    {

#line 1120
        if(i_0 < (_S16.group_count_0))
        {
        }
        else
        {

#line 1120
            break;
        }
        uint at_2 = _S16.first_group_0 + i_0;

#line 1122
        LevelGroup_0 _S21 = level_group_at_0(at_2, kernelContext_16);

#line 1127
        uint _S22 = _S20 + at_2;

#line 1127
        uint _S23 = group_is_expanded_0(_S21.error_0 * _S19, (((float4(_S21.center_x_0, _S21.center_y_0, _S21.center_z_0, 1.0f)) * (_S18))).xyz, _S21.radius_0 * _S19, _S17, *(kernelContext_16->group_state_0+_S22), kernelContext_16);
        *(kernelContext_16->group_state_0+_S22) = _S23;

#line 1128
        bool _S24;
        if(_S23 == 1U)
        {

#line 1129
            _S24 = (_S21.level_0) < chosen_0;

#line 1129
        }
        else
        {

#line 1129
            _S24 = false;

#line 1129
        }

#line 1129
        if(_S24)
        {

#line 1129
            chosen_0 = _S21.level_0;

#line 1129
        }

#line 1120
        i_0 = i_0 + 1U;

#line 1120
    }

#line 1134
    return chosen_0;
}


#line 1134
uint instance_material_mode_0(uint _S25, KernelContext_0 thread* kernelContext_17)
{

#line 823
    return (((kernelContext_17->instances_0+_S25)->flags_1) & 12U) >> 2U;
}


#line 1158
[[kernel]] void binMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* tables_1 [[buffer(4)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], uint device* visible_instances_1 [[buffer(5)]], atomic<uint> device* args_1 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_1 [[buffer(7)]], uint device* visible_count_1 [[buffer(3)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], uint device* group_state_1 [[buffer(8)]])
{

#line 1158
    thread KernelContext_0 kernelContext_18;

#line 1158
    (&kernelContext_18)->gen_0 = gen_1;

#line 1158
    (&kernelContext_18)->tables_0 = tables_1;

#line 1158
    (&kernelContext_18)->meshes_0 = meshes_1;

#line 1158
    (&kernelContext_18)->visible_instances_0 = visible_instances_1;

#line 1158
    (&kernelContext_18)->args_0 = args_1;

#line 1158
    (&kernelContext_18)->counts_and_mesh_args_0 = counts_and_mesh_args_1;

#line 1158
    (&kernelContext_18)->visible_count_0 = visible_count_1;

#line 1158
    (&kernelContext_18)->instances_0 = instances_1;

#line 1158
    (&kernelContext_18)->group_state_0 = group_state_1;

#line 1158
    threadgroup array<uint, int(256)> starts_chunk_runs_1;

#line 1158
    (&kernelContext_18)->starts_chunk_runs_0 = &starts_chunk_runs_1;

    uint index_0 = thread_0.x;

#line 1160
    uint face_0;

#line 1165
    if(index_0 < (gen_1->bucket_count_0))
    {

#line 1165
        uint _S26 = bucket_mesh_0(index_0, &kernelContext_18);

        GpuMesh_0 _S27 = (&kernelContext_18)->meshes_0[_S26];

#line 1167
        uint _S28 = bucket_mesh_word_0(index_0, &kernelContext_18);

#line 1172
        uint device* _S29 = (&kernelContext_18)->visible_instances_0+_S28;

#line 1172
        uint _S30 = bucket_mesh_0(index_0, &kernelContext_18);

#line 1172
        *_S29 = _S30;

#line 1172
        face_0 = 0U;


        for(;;)
        {

#line 1175
            if(face_0 < ((&kernelContext_18)->gen_0->draw_regions_0))
            {
            }
            else
            {

#line 1175
                break;
            }

#line 1175
            uint _S31 = arg_word_0(face_0, index_0, 0U, &kernelContext_18);

            atomic_store_explicit((&kernelContext_18)->args_0+_S31, _S27.index_count_0, memory_order_relaxed);

#line 1177
            uint _S32 = arg_word_0(face_0, index_0, 2U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->args_0+_S32, _S27.base_index_0, memory_order_relaxed);

#line 1178
            uint _S33 = arg_word_0(face_0, index_0, 3U, &kernelContext_18);

#line 1185
            atomic_store_explicit((&kernelContext_18)->args_0+_S33, 0U, memory_order_relaxed);

#line 1185
            uint _S34 = arg_word_0(face_0, index_0, 4U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->args_0+_S34, 0U, memory_order_relaxed);

#line 1186
            uint _S35 = mesh_arg_word_0(face_0, index_0, 0U, &kernelContext_18);

#line 1193
            atomic<uint> device* _S36 = (&kernelContext_18)->counts_and_mesh_args_0+_S35;

#line 1193
            uint _S37 = bucket_clusters_0(index_0, &kernelContext_18);

#line 1193
            atomic_store_explicit(_S36, _S37, memory_order_relaxed);

#line 1193
            uint _S38 = mesh_arg_word_0(face_0, index_0, 2U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S38, 1U, memory_order_relaxed);

#line 1175
            face_0 = face_0 + 1U;

#line 1175
        }

#line 1165
    }

#line 1165
    uint _S39 = survivor_count_0(&kernelContext_18);

#line 1200
    if(index_0 >= _S39)
    {
        return;
    }



    uint device* _S40 = (&kernelContext_18)->visible_instances_0+index_0;

#line 1207
    uint entry_0 = *_S40;


    uint instance_index_0 = (*_S40) & 16777215U;

#line 1210
    MeshLevels_0 _S41 = mesh_levels_of_0(((&kernelContext_18)->instances_0+instance_index_0)->mesh_0, &kernelContext_18);

#line 1210
    uint _S42 = select_level_0(instance_index_0, instance_index_0, &kernelContext_18);

#line 1210
    uint _S43 = level_mesh_at_0(_S41.first_level_0 + _S42, &kernelContext_18);

#line 1210
    uint _S44 = instance_material_mode_0(instance_index_0, &kernelContext_18);

#line 1210
    uint _S45 = bucket_for_0(_S43, _S44, &kernelContext_18);

#line 1229
    if(_S45 != 4294967295U)
    {

#line 1238
        if(((&kernelContext_18)->gen_0->mode_0) == 2U)
        {

#line 1238
            face_0 = 0U;


            for(;;)
            {

#line 1241
                if(face_0 < 6U)
                {
                }
                else
                {

#line 1241
                    break;
                }
                if(((entry_0 >> (24U + face_0)) & 1U) != 0U)
                {

#line 1243
                    uint _S46 = mesh_arg_word_0(1U + face_0, _S45, 1U, &kernelContext_18);


                    uint _S47 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S46, 1U, memory_order_relaxed);

#line 1243
                }

#line 1241
                face_0 = face_0 + 1U;

#line 1241
            }

#line 1238
        }
        else
        {

#line 1238
            uint _S48 = mesh_arg_word_0(0U, _S45, 1U, &kernelContext_18);

#line 1255
            uint _S49 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S48, 1U, memory_order_relaxed);

#line 1255
            bool _S50;
            if(((&kernelContext_18)->gen_0->mode_0) == 1U)
            {

#line 1256
                _S50 = (entry_0 & 2147483648U) == 0U;

#line 1256
            }
            else
            {

#line 1256
                _S50 = false;

#line 1256
            }

#line 1256
            if(_S50)
            {

#line 1256
                uint _S51 = mesh_arg_word_0(1U, _S45, 1U, &kernelContext_18);


                uint _S52 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S51, 1U, memory_order_relaxed);

#line 1256
            }

#line 1238
        }

#line 1229
    }

#line 1229
    uint _S53 = route_word_0(index_0, &kernelContext_18);

#line 1266
    *((&kernelContext_18)->visible_instances_0+_S53) = _S45;
    return;
}


#line 1291
uint starts_slot_count_0(KernelContext_0 thread* kernelContext_19)
{

#line 1291
    uint _S54;

    if((kernelContext_19->gen_0->mode_0) == 2U)
    {

#line 1293
        _S54 = 6U * kernelContext_19->gen_0->bucket_count_0;

#line 1293
    }
    else
    {

#line 1293
        _S54 = kernelContext_19->gen_0->bucket_count_0;

#line 1293
    }

#line 1293
    return _S54;
}


uint starts_slot_region_0(uint slot_1, KernelContext_0 thread* kernelContext_20)
{

#line 1297
    uint _S55;

    if((kernelContext_20->gen_0->mode_0) == 2U)
    {

#line 1299
        uint _S56 = slot_1 / kernelContext_20->gen_0->bucket_count_0;

#line 1299
        _S55 = 1U + _S56;

#line 1299
    }
    else
    {

#line 1299
        _S55 = 0U;

#line 1299
    }

#line 1299
    return _S55;
}


uint starts_slot_bucket_0(uint slot_2, KernelContext_0 thread* kernelContext_21)
{

#line 1303
    uint _S57;

    if((kernelContext_21->gen_0->mode_0) == 2U)
    {

#line 1305
        uint _S58 = slot_2 % kernelContext_21->gen_0->bucket_count_0;

#line 1305
        _S57 = _S58;

#line 1305
    }
    else
    {

#line 1305
        _S57 = slot_2;

#line 1305
    }

#line 1305
    return _S57;
}



uint starts_slot_runs_0(uint slot_3, KernelContext_0 thread* kernelContext_22)
{

#line 1310
    uint _S59 = starts_slot_region_0(slot_3, kernelContext_22);

#line 1310
    uint _S60 = starts_slot_bucket_0(slot_3, kernelContext_22);

#line 1310
    uint _S61 = mesh_arg_word_0(_S59, _S60, 1U, kernelContext_22);


    uint _S62 = atomic_load_explicit(kernelContext_22->counts_and_mesh_args_0+_S61, memory_order_relaxed);

#line 1312
    return _S62;
}


#line 923
uint runs_at_0(KernelContext_0 thread* kernelContext_23)
{
    return 2U * kernelContext_23->gen_0->visible_capacity_0;
}


#line 965
uint count_word_0(uint region_6, uint bucket_9, KernelContext_0 thread* kernelContext_24)
{
    return region_6 * 4U * kernelContext_24->gen_0->bucket_count_0 + bucket_9;
}


#line 1336
void write_task_extents_0(uint region_7, uint bucket_10, uint instances_2, KernelContext_0 thread* kernelContext_25)
{
    uint lanes_0 = kernelContext_25->gen_0->task_lanes_0;
    if((kernelContext_25->gen_0->task_lanes_0) == 0U)
    {
        return;
    }

#line 1341
    uint _S63 = bucket_clusters_0(bucket_10, kernelContext_25);


    uint _S64 = _S63 / lanes_0;

#line 1344
    uint _S65 = _S64 * instances_2;

#line 1344
    uint _S66 = _S63 % lanes_0;

#line 1344
    uint _S67 = (_S66 * instances_2 + lanes_0 - 1U) / lanes_0;

#line 1344
    uint chunks_0 = _S65 + _S67;

#line 1344
    uint _S68 = mesh_arg_word_0(region_7, bucket_10, 0U, kernelContext_25);

    atomic_store_explicit(kernelContext_25->counts_and_mesh_args_0+_S68, min(chunks_0, 65535U), memory_order_relaxed);

#line 1346
    uint _S69 = mesh_arg_word_0(region_7, bucket_10, 1U, kernelContext_25);

    atomic_store_explicit(kernelContext_25->counts_and_mesh_args_0+_S69, (chunks_0 + 65535U - 1U) / 65535U, memory_order_relaxed);
    return;
}


#line 1374
[[kernel]] void startsMain(uint3 thread_1 [[thread_position_in_threadgroup]], DrawGenParams_0 constant* gen_2 [[buffer(0)]], uint device* tables_2 [[buffer(4)]], GpuMesh_0 device* meshes_2 [[buffer(2)]], uint device* visible_instances_2 [[buffer(5)]], atomic<uint> device* args_2 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_2 [[buffer(7)]], uint device* visible_count_2 [[buffer(3)]], GpuInstance_natural_0 device* instances_3 [[buffer(1)]], uint device* group_state_2 [[buffer(8)]])
{

#line 1374
    uint behind_0;

#line 1374
    thread KernelContext_0 kernelContext_26;

#line 1374
    (&kernelContext_26)->gen_0 = gen_2;

#line 1374
    (&kernelContext_26)->tables_0 = tables_2;

#line 1374
    (&kernelContext_26)->meshes_0 = meshes_2;

#line 1374
    (&kernelContext_26)->visible_instances_0 = visible_instances_2;

#line 1374
    (&kernelContext_26)->args_0 = args_2;

#line 1374
    (&kernelContext_26)->counts_and_mesh_args_0 = counts_and_mesh_args_2;

#line 1374
    (&kernelContext_26)->visible_count_0 = visible_count_2;

#line 1374
    (&kernelContext_26)->instances_0 = instances_3;

#line 1374
    (&kernelContext_26)->group_state_0 = group_state_2;

#line 1374
    threadgroup array<uint, int(256)> starts_chunk_runs_2;

#line 1374
    (&kernelContext_26)->starts_chunk_runs_0 = &starts_chunk_runs_2;

    uint lane_0 = thread_1.x;

#line 1376
    uint _S70 = starts_slot_count_0(&kernelContext_26);

    uint chunk_0 = (_S70 + 256U - 1U) / 256U;
    uint _S71 = min(lane_0 * chunk_0, _S70);
    uint _S72 = min(_S71 + chunk_0, _S70);

#line 1380
    uint slot_4 = _S71;

#line 1380
    uint total_0 = 0U;


    for(;;)
    {

#line 1383
        if(slot_4 < _S72)
        {
        }
        else
        {

#line 1383
            break;
        }

#line 1383
        uint _S73 = starts_slot_runs_0(slot_4, &kernelContext_26);

        uint total_1 = total_0 + _S73;

#line 1383
        slot_4 = slot_4 + 1U;

#line 1383
        total_0 = total_1;

#line 1383
    }

#line 1391
    (*(&kernelContext_26)->starts_chunk_runs_0)[lane_0] = total_0;
    threadgroup_barrier(mem_flags::mem_threadgroup);

#line 1392
    uint reach_0 = 1U;
    for(;;)
    {

#line 1393
        if(reach_0 < 256U)
        {
        }
        else
        {

#line 1393
            break;
        }
        if(lane_0 >= reach_0)
        {

#line 1395
            behind_0 = (*(&kernelContext_26)->starts_chunk_runs_0)[lane_0 - reach_0];

#line 1395
        }
        else
        {

#line 1395
            behind_0 = 0U;

#line 1395
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
        (*(&kernelContext_26)->starts_chunk_runs_0)[lane_0] = (*(&kernelContext_26)->starts_chunk_runs_0)[lane_0] + behind_0;
        threadgroup_barrier(mem_flags::mem_threadgroup);

#line 1393
        reach_0 = reach_0 << 1U;

#line 1393
    }

#line 1404
    if(((&kernelContext_26)->gen_0->mode_0) == 2U)
    {

#line 1404
        slot_4 = (&kernelContext_26)->gen_0->face_runs_at_0;

#line 1404
    }
    else
    {

#line 1404
        uint _S74 = runs_at_0(&kernelContext_26);

#line 1404
        slot_4 = _S74;

#line 1404
    }
    uint _S75 = slot_4 + (*(&kernelContext_26)->starts_chunk_runs_0)[lane_0] - total_0;

#line 1405
    slot_4 = _S71;

#line 1405
    behind_0 = _S75;
    for(;;)
    {

#line 1406
        if(slot_4 < _S72)
        {
        }
        else
        {

#line 1406
            break;
        }

#line 1406
        uint _S76 = starts_slot_region_0(slot_4, &kernelContext_26);

#line 1406
        uint _S77 = starts_slot_bucket_0(slot_4, &kernelContext_26);

#line 1406
        uint _S78 = starts_slot_runs_0(slot_4, &kernelContext_26);

#line 1406
        uint _S79 = run_start_word_0(_S76, _S77, &kernelContext_26);

#line 1411
        *((&kernelContext_26)->visible_instances_0+_S79) = behind_0;


        if(_S78 != 0U)
        {

#line 1414
            uint _S80 = count_word_0(_S76, _S77, &kernelContext_26);

            atomic_store_explicit((&kernelContext_26)->counts_and_mesh_args_0+_S80, 1U, memory_order_relaxed);

#line 1414
        }

#line 1414
        write_task_extents_0(_S76, _S77, _S78, &kernelContext_26);

#line 1422
        if(((&kernelContext_26)->gen_0->mode_0) == 1U)
        {

#line 1422
            uint _S81 = mesh_arg_word_0(1U, _S77, 1U, &kernelContext_26);

#line 1428
            uint early_0 = atomic_load_explicit((&kernelContext_26)->counts_and_mesh_args_0+_S81, memory_order_relaxed);

#line 1428
            uint _S82 = run_start_word_0(1U, _S77, &kernelContext_26);
            *((&kernelContext_26)->visible_instances_0+_S82) = behind_0;

#line 1429
            uint _S83 = run_start_word_0(2U, _S77, &kernelContext_26);
            *((&kernelContext_26)->visible_instances_0+_S83) = behind_0 + early_0;
            if(early_0 != 0U)
            {

#line 1431
                uint _S84 = count_word_0(1U, _S77, &kernelContext_26);

                atomic_store_explicit((&kernelContext_26)->counts_and_mesh_args_0+_S84, 1U, memory_order_relaxed);

#line 1431
            }

#line 1431
            write_task_extents_0(1U, _S77, early_0, &kernelContext_26);

#line 1422
        }

#line 1437
        uint start_0 = behind_0 + _S78;

#line 1406
        slot_4 = slot_4 + 1U;

#line 1406
        behind_0 = start_0;

#line 1406
    }

#line 1439
    return;
}


#line 1450
[[kernel]] void scatterMain(uint3 thread_2 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_3 [[buffer(0)]], uint device* tables_3 [[buffer(4)]], GpuMesh_0 device* meshes_3 [[buffer(2)]], uint device* visible_instances_3 [[buffer(5)]], atomic<uint> device* args_3 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_3 [[buffer(7)]], uint device* visible_count_3 [[buffer(3)]], GpuInstance_natural_0 device* instances_4 [[buffer(1)]], uint device* group_state_3 [[buffer(8)]])
{

#line 1450
    thread KernelContext_0 kernelContext_27;

#line 1450
    (&kernelContext_27)->gen_0 = gen_3;

#line 1450
    (&kernelContext_27)->tables_0 = tables_3;

#line 1450
    (&kernelContext_27)->meshes_0 = meshes_3;

#line 1450
    (&kernelContext_27)->visible_instances_0 = visible_instances_3;

#line 1450
    (&kernelContext_27)->args_0 = args_3;

#line 1450
    (&kernelContext_27)->counts_and_mesh_args_0 = counts_and_mesh_args_3;

#line 1450
    (&kernelContext_27)->visible_count_0 = visible_count_3;

#line 1450
    (&kernelContext_27)->instances_0 = instances_4;

#line 1450
    (&kernelContext_27)->group_state_0 = group_state_3;

#line 1450
    threadgroup array<uint, int(256)> starts_chunk_runs_3;

#line 1450
    (&kernelContext_27)->starts_chunk_runs_0 = &starts_chunk_runs_3;

    uint index_1 = thread_2.x;

#line 1452
    uint _S85 = survivor_count_0(&kernelContext_27);
    if(index_1 >= _S85)
    {
        return;
    }

#line 1455
    uint _S86 = route_word_0(index_1, &kernelContext_27);

    uint device* _S87 = (&kernelContext_27)->visible_instances_0+_S86;

#line 1457
    uint bucket_11 = *_S87;
    if((*_S87) == 4294967295U)
    {
        return;
    }
    uint device* _S88 = (&kernelContext_27)->visible_instances_0+index_1;

#line 1462
    uint entry_1 = *_S88;
    uint instance_index_1 = (*_S88) & 16777215U;

#line 1463
    uint face_1;
    if(((&kernelContext_27)->gen_0->mode_0) == 2U)
    {

#line 1464
        face_1 = 0U;

        for(;;)
        {

#line 1466
            if(face_1 < 6U)
            {
            }
            else
            {

#line 1466
                break;
            }
            if(((entry_1 >> (24U + face_1)) & 1U) == 0U)
            {
                face_1 = face_1 + 1U;

#line 1466
                continue;
            }

#line 1472
            uint region_8 = 1U + face_1;

#line 1472
            uint _S89 = arg_word_0(region_8, bucket_11, 1U, &kernelContext_27);
            uint face_slot_0 = atomic_fetch_add_explicit((&kernelContext_27)->args_0+_S89, 1U, memory_order_relaxed);

#line 1473
            uint _S90 = run_start_word_0(region_8, bucket_11, &kernelContext_27);
            *((&kernelContext_27)->visible_instances_0+(*((&kernelContext_27)->visible_instances_0+_S90) + face_slot_0)) = instance_index_1;

#line 1466
            face_1 = face_1 + 1U;

#line 1466
        }

#line 1477
        return;
    }

#line 1477
    bool _S91;


    if(((&kernelContext_27)->gen_0->mode_0) == 1U)
    {

#line 1480
        _S91 = (entry_1 & 2147483648U) != 0U;

#line 1480
    }
    else
    {

#line 1480
        _S91 = false;

#line 1480
    }

#line 1480
    if(_S91)
    {
        return;
    }
    if(((&kernelContext_27)->gen_0->mode_0) == 1U)
    {

#line 1484
        face_1 = 1U;

#line 1484
    }
    else
    {

#line 1484
        face_1 = 0U;

#line 1484
    }

#line 1484
    uint _S92 = arg_word_0(face_1, bucket_11, 1U, &kernelContext_27);
    uint slot_5 = atomic_fetch_add_explicit((&kernelContext_27)->args_0+_S92, 1U, memory_order_relaxed);

#line 1485
    uint _S93 = run_start_word_0(face_1, bucket_11, &kernelContext_27);
    *((&kernelContext_27)->visible_instances_0+(*((&kernelContext_27)->visible_instances_0+_S93) + slot_5)) = instance_index_1;
    return;
}


#line 1498
[[kernel]] void lateScatterMain(uint3 thread_3 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_4 [[buffer(0)]], uint device* tables_4 [[buffer(4)]], GpuMesh_0 device* meshes_4 [[buffer(2)]], uint device* visible_instances_4 [[buffer(5)]], atomic<uint> device* args_4 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_4 [[buffer(7)]], uint device* visible_count_4 [[buffer(3)]], GpuInstance_natural_0 device* instances_5 [[buffer(1)]], uint device* group_state_4 [[buffer(8)]])
{

#line 1498
    thread KernelContext_0 kernelContext_28;

#line 1498
    (&kernelContext_28)->gen_0 = gen_4;

#line 1498
    (&kernelContext_28)->tables_0 = tables_4;

#line 1498
    (&kernelContext_28)->meshes_0 = meshes_4;

#line 1498
    (&kernelContext_28)->visible_instances_0 = visible_instances_4;

#line 1498
    (&kernelContext_28)->args_0 = args_4;

#line 1498
    (&kernelContext_28)->counts_and_mesh_args_0 = counts_and_mesh_args_4;

#line 1498
    (&kernelContext_28)->visible_count_0 = visible_count_4;

#line 1498
    (&kernelContext_28)->instances_0 = instances_5;

#line 1498
    (&kernelContext_28)->group_state_0 = group_state_4;

#line 1498
    threadgroup array<uint, int(256)> starts_chunk_runs_4;

#line 1498
    (&kernelContext_28)->starts_chunk_runs_0 = &starts_chunk_runs_4;

    uint index_2 = thread_3.x;

#line 1500
    uint _S94 = survivor_count_0(&kernelContext_28);
    if(index_2 >= _S94)
    {
        return;
    }
    uint device* _S95 = (&kernelContext_28)->visible_instances_0+index_2;

#line 1505
    uint entry_2 = *_S95;
    if(((*_S95) & 1073741824U) == 0U)
    {
        return;
    }

#line 1508
    uint _S96 = route_word_0(index_2, &kernelContext_28);

    uint device* _S97 = (&kernelContext_28)->visible_instances_0+_S96;

#line 1510
    uint bucket_12 = *_S97;
    if((*_S97) == 4294967295U)
    {
        return;
    }

#line 1513
    uint _S98 = arg_word_0(2U, bucket_12, 1U, &kernelContext_28);

    uint slot_6 = atomic_fetch_add_explicit((&kernelContext_28)->args_0+_S98, 1U, memory_order_relaxed);

#line 1515
    uint _S99 = run_start_word_0(2U, bucket_12, &kernelContext_28);
    *((&kernelContext_28)->visible_instances_0+(*((&kernelContext_28)->visible_instances_0+_S99) + slot_6)) = entry_2 & 16777215U;

    return;
}


#line 1529
[[kernel]] void lateFinishMain(uint3 thread_4 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_5 [[buffer(0)]], uint device* tables_5 [[buffer(4)]], GpuMesh_0 device* meshes_5 [[buffer(2)]], uint device* visible_instances_5 [[buffer(5)]], atomic<uint> device* args_5 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_5 [[buffer(7)]], uint device* visible_count_5 [[buffer(3)]], GpuInstance_natural_0 device* instances_6 [[buffer(1)]], uint device* group_state_5 [[buffer(8)]])
{

#line 1529
    thread KernelContext_0 kernelContext_29;

#line 1529
    (&kernelContext_29)->gen_0 = gen_5;

#line 1529
    (&kernelContext_29)->tables_0 = tables_5;

#line 1529
    (&kernelContext_29)->meshes_0 = meshes_5;

#line 1529
    (&kernelContext_29)->visible_instances_0 = visible_instances_5;

#line 1529
    (&kernelContext_29)->args_0 = args_5;

#line 1529
    (&kernelContext_29)->counts_and_mesh_args_0 = counts_and_mesh_args_5;

#line 1529
    (&kernelContext_29)->visible_count_0 = visible_count_5;

#line 1529
    (&kernelContext_29)->instances_0 = instances_6;

#line 1529
    (&kernelContext_29)->group_state_0 = group_state_5;

#line 1529
    threadgroup array<uint, int(256)> starts_chunk_runs_5;

#line 1529
    (&kernelContext_29)->starts_chunk_runs_0 = &starts_chunk_runs_5;

    uint bucket_13 = thread_4.x;
    if(bucket_13 >= (gen_5->bucket_count_0))
    {
        return;
    }

#line 1534
    uint _S100 = arg_word_0(1U, bucket_13, 1U, &kernelContext_29);

    uint early_1 = atomic_load_explicit((&kernelContext_29)->args_0+_S100, memory_order_relaxed);

#line 1536
    uint _S101 = arg_word_0(2U, bucket_13, 1U, &kernelContext_29);
    uint late_0 = atomic_load_explicit((&kernelContext_29)->args_0+_S101, memory_order_relaxed);

#line 1537
    uint _S102 = mesh_arg_word_0(2U, bucket_13, 1U, &kernelContext_29);
    atomic_store_explicit((&kernelContext_29)->counts_and_mesh_args_0+_S102, late_0, memory_order_relaxed);
    if(late_0 != 0U)
    {

#line 1539
        uint _S103 = count_word_0(2U, bucket_13, &kernelContext_29);

        atomic_store_explicit((&kernelContext_29)->counts_and_mesh_args_0+_S103, 1U, memory_order_relaxed);

#line 1539
    }



    uint drawn_0 = early_1 + late_0;

#line 1543
    uint _S104 = arg_word_0(0U, bucket_13, 1U, &kernelContext_29);
    atomic_store_explicit((&kernelContext_29)->args_0+_S104, drawn_0, memory_order_relaxed);

#line 1544
    uint _S105 = mesh_arg_word_0(0U, bucket_13, 1U, &kernelContext_29);
    atomic_store_explicit((&kernelContext_29)->counts_and_mesh_args_0+_S105, drawn_0, memory_order_relaxed);

#line 1545
    uint _S106 = count_word_0(0U, bucket_13, &kernelContext_29);
    atomic<uint> device* _S107 = (&kernelContext_29)->counts_and_mesh_args_0+_S106;

#line 1546
    int _S108;

#line 1546
    if(drawn_0 != 0U)
    {

#line 1546
        _S108 = int(1);

#line 1546
    }
    else
    {

#line 1546
        _S108 = int(0);

#line 1546
    }

#line 1546
    atomic_store_explicit(_S107, uint(_S108), memory_order_relaxed);

#line 1546
    write_task_extents_0(2U, bucket_13, late_0, &kernelContext_29);

#line 1546
    write_task_extents_0(0U, bucket_13, drawn_0, &kernelContext_29);

#line 1551
    return;
}

