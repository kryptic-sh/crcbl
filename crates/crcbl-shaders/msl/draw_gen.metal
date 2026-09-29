#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 543 "shaders/draw_gen.slang"
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
    uint flat_segments_at_0;
};


#line 327
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


#line 659
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 659
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


#line 1429
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


#line 828
uint bucket_mesh_0(uint bucket_0, KernelContext_0 thread* kernelContext_0)
{
    return kernelContext_0->tables_0[bucket_0];
}


#line 951
uint draw_slot_0(uint region_0, uint bucket_1, KernelContext_0 thread* kernelContext_1)
{
    return region_0 * kernelContext_1->gen_0->bucket_count_0 + bucket_1;
}


#line 951
uint draw_slot_1(uint region_1, uint bucket_2, KernelContext_0 thread* kernelContext_2)
{
    return region_1 * kernelContext_2->gen_0->bucket_count_0 + bucket_2;
}


#line 962
uint run_start_word_0(uint region_2, uint bucket_3, KernelContext_0 thread* kernelContext_3)
{
    uint _S1 = 3U * kernelContext_3->gen_0->visible_capacity_0;

#line 964
    uint _S2 = draw_slot_0(region_2, bucket_3, kernelContext_3);

#line 964
    return _S1 + _S2;
}


#line 962
uint run_start_word_1(uint region_3, uint bucket_4, KernelContext_0 thread* kernelContext_4)
{
    uint _S3 = 3U * kernelContext_4->gen_0->visible_capacity_0;

#line 964
    uint _S4 = draw_slot_0(region_3, bucket_4, kernelContext_4);

#line 964
    return _S3 + _S4;
}


#line 976
uint bucket_mesh_word_0(uint bucket_5, KernelContext_0 thread* kernelContext_5)
{

#line 976
    uint _S5 = run_start_word_1(7U, bucket_5, kernelContext_5);

    return _S5;
}


#line 998
uint arg_word_0(uint region_4, uint bucket_6, uint field_0, KernelContext_0 thread* kernelContext_6)
{

#line 998
    uint _S6 = draw_slot_1(region_4, bucket_6, kernelContext_6);

    return _S6 * 5U + field_0;
}


#line 991
uint mesh_arg_word_0(uint region_5, uint bucket_7, uint slot_0, KernelContext_0 thread* kernelContext_7)
{
    return region_5 * 4U * kernelContext_7->gen_0->bucket_count_0 + kernelContext_7->gen_0->bucket_count_0 + bucket_7 * 3U + slot_0;
}


#line 851
uint bucket_clusters_0(uint bucket_8, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->tables_0[kernelContext_8->gen_0->bucket_clusters_at_0 + bucket_8];
}


#line 1192
uint survivor_count_0(KernelContext_0 thread* kernelContext_9)
{
    return min(kernelContext_9->visible_count_0[0U], kernelContext_9->gen_0->visible_capacity_0);
}


#line 508
struct MeshLevels_0
{
    uint first_group_0;
    uint group_count_0;
    uint first_level_0;
    uint top_level_0;
};


#line 858
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


#line 1099
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S7 = (((basis_0) * (transpose(basis_0))));

#line 1101
    float bound_0 = 0.0f;

#line 1101
    uint row_0 = 0U;

    for(;;)
    {

#line 1103
        if(row_0 < 3U)
        {
        }
        else
        {

#line 1103
            break;
        }
        float _S8 = max(bound_0, abs(_S7[row_0][int(0)]) + abs(_S7[row_0][int(1)]) + abs(_S7[row_0][int(2)]));

#line 1103
        uint row_1 = row_0 + 1U;

#line 1103
        bound_0 = _S8;

#line 1103
        row_0 = row_1;

#line 1103
    }



    return sqrt(bound_0);
}


#line 463
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_0;
    float center_y_0;
    float center_z_0;
    float radius_0;
};


#line 876
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


#line 1065
float projected_error_0(float error_1, float3 center_0, float radius_1, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S9 = delta_0.x;

#line 1068
    float _S10 = delta_0.y;

#line 1068
    float _S11 = delta_0.z;
    float distance_0 = sqrt(_S9 * _S9 + _S10 * _S10 + _S11 * _S11) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 1119
uint group_is_expanded_0(float error_2, float3 center_1, float radius_2, float3 eye_1, uint was_0, KernelContext_0 thread* kernelContext_12)
{
    float projected_0 = projected_error_0(error_2, center_1, radius_2, eye_1, kernelContext_12->gen_0->lod_params_0.x);

#line 1121
    bool expanded_0;

    if(projected_0 > (kernelContext_12->gen_0->lod_params_0.y))
    {

#line 1123
        expanded_0 = true;

#line 1123
    }
    else
    {

#line 1123
        if(was_0 != 0U)
        {

#line 1123
            expanded_0 = projected_0 > (kernelContext_12->gen_0->lod_params_0.z);

#line 1123
        }
        else
        {

#line 1123
            expanded_0 = false;

#line 1123
        }

#line 1123
    }

#line 1123
    uint _S12;
    if(expanded_0)
    {

#line 1124
        _S12 = 1U;

#line 1124
    }
    else
    {

#line 1124
        _S12 = 0U;

#line 1124
    }

#line 1124
    return _S12;
}


#line 895
uint level_mesh_at_0(uint level_1, KernelContext_0 thread* kernelContext_13)
{
    return kernelContext_13->tables_0[kernelContext_13->gen_0->level_meshes_at_0 + level_1];
}


#line 924
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


#line 1207
uint select_level_0(uint _S13, uint _S14, KernelContext_0 thread* kernelContext_16)
{

#line 1207
    GpuInstance_natural_0 device* _S15 = kernelContext_16->instances_0+_S13;

#line 1207
    MeshLevels_0 _S16 = mesh_levels_of_0(_S15->mesh_0, kernelContext_16);

#line 1165
    float3 _S17 = kernelContext_16->gen_0->camera_position_0.xyz;

#line 1165
    matrix<float,int(4),int(4)>  _S18 = matrix<float,int(4),int(4)> (_S15->transform_0.data_0[int(0)][int(0)], _S15->transform_0.data_0[int(1)][int(0)], _S15->transform_0.data_0[int(2)][int(0)], _S15->transform_0.data_0[int(3)][int(0)], _S15->transform_0.data_0[int(0)][int(1)], _S15->transform_0.data_0[int(1)][int(1)], _S15->transform_0.data_0[int(2)][int(1)], _S15->transform_0.data_0[int(3)][int(1)], _S15->transform_0.data_0[int(0)][int(2)], _S15->transform_0.data_0[int(1)][int(2)], _S15->transform_0.data_0[int(2)][int(2)], _S15->transform_0.data_0[int(3)][int(2)], _S15->transform_0.data_0[int(0)][int(3)], _S15->transform_0.data_0[int(1)][int(3)], _S15->transform_0.data_0[int(2)][int(3)], _S15->transform_0.data_0[int(3)][int(3)]);
    float _S19 = max_stretch_0(matrix<float,int(3),int(3)> (_S18[int(0)].xyz, _S18[int(1)].xyz, _S18[int(2)].xyz));
    uint _S20 = _S14 * kernelContext_16->gen_0->group_stride_0;

#line 1167
    uint chosen_0 = _S16.top_level_0;

#line 1167
    uint i_0 = 0U;

    for(;;)
    {

#line 1169
        if(i_0 < (_S16.group_count_0))
        {
        }
        else
        {

#line 1169
            break;
        }
        uint at_2 = _S16.first_group_0 + i_0;

#line 1171
        LevelGroup_0 _S21 = level_group_at_0(at_2, kernelContext_16);

#line 1176
        uint _S22 = _S20 + at_2;

#line 1176
        uint _S23 = group_is_expanded_0(_S21.error_0 * _S19, (((float4(_S21.center_x_0, _S21.center_y_0, _S21.center_z_0, 1.0f)) * (_S18))).xyz, _S21.radius_0 * _S19, _S17, *(kernelContext_16->group_state_0+_S22), kernelContext_16);
        *(kernelContext_16->group_state_0+_S22) = _S23;

#line 1177
        bool _S24;
        if(_S23 == 1U)
        {

#line 1178
            _S24 = (_S21.level_0) < chosen_0;

#line 1178
        }
        else
        {

#line 1178
            _S24 = false;

#line 1178
        }

#line 1178
        if(_S24)
        {

#line 1178
            chosen_0 = _S21.level_0;

#line 1178
        }

#line 1169
        i_0 = i_0 + 1U;

#line 1169
    }

#line 1183
    return chosen_0;
}


#line 1183
uint instance_material_mode_0(uint _S25, KernelContext_0 thread* kernelContext_17)
{

#line 842
    return (((kernelContext_17->instances_0+_S25)->flags_1) & 12U) >> 2U;
}


#line 1207
[[kernel]] void binMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* tables_1 [[buffer(4)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], uint device* visible_instances_1 [[buffer(5)]], atomic<uint> device* args_1 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_1 [[buffer(7)]], uint device* visible_count_1 [[buffer(3)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], uint device* group_state_1 [[buffer(8)]])
{

#line 1207
    thread KernelContext_0 kernelContext_18;

#line 1207
    (&kernelContext_18)->gen_0 = gen_1;

#line 1207
    (&kernelContext_18)->tables_0 = tables_1;

#line 1207
    (&kernelContext_18)->meshes_0 = meshes_1;

#line 1207
    (&kernelContext_18)->visible_instances_0 = visible_instances_1;

#line 1207
    (&kernelContext_18)->args_0 = args_1;

#line 1207
    (&kernelContext_18)->counts_and_mesh_args_0 = counts_and_mesh_args_1;

#line 1207
    (&kernelContext_18)->visible_count_0 = visible_count_1;

#line 1207
    (&kernelContext_18)->instances_0 = instances_1;

#line 1207
    (&kernelContext_18)->group_state_0 = group_state_1;

#line 1207
    threadgroup array<uint, int(256)> starts_chunk_runs_1;

#line 1207
    (&kernelContext_18)->starts_chunk_runs_0 = &starts_chunk_runs_1;

    uint index_0 = thread_0.x;

#line 1209
    uint face_0;

#line 1214
    if(index_0 < (gen_1->bucket_count_0))
    {

#line 1214
        uint _S26 = bucket_mesh_0(index_0, &kernelContext_18);

        GpuMesh_0 _S27 = (&kernelContext_18)->meshes_0[_S26];

#line 1216
        uint _S28 = bucket_mesh_word_0(index_0, &kernelContext_18);

#line 1221
        uint device* _S29 = (&kernelContext_18)->visible_instances_0+_S28;

#line 1221
        uint _S30 = bucket_mesh_0(index_0, &kernelContext_18);

#line 1221
        *_S29 = _S30;

#line 1221
        face_0 = 0U;


        for(;;)
        {

#line 1224
            if(face_0 < ((&kernelContext_18)->gen_0->draw_regions_0))
            {
            }
            else
            {

#line 1224
                break;
            }

#line 1224
            uint _S31 = arg_word_0(face_0, index_0, 0U, &kernelContext_18);

            atomic_store_explicit((&kernelContext_18)->args_0+_S31, _S27.index_count_0, memory_order_relaxed);

#line 1226
            uint _S32 = arg_word_0(face_0, index_0, 2U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->args_0+_S32, _S27.base_index_0, memory_order_relaxed);

#line 1227
            uint _S33 = arg_word_0(face_0, index_0, 3U, &kernelContext_18);

#line 1234
            atomic_store_explicit((&kernelContext_18)->args_0+_S33, 0U, memory_order_relaxed);

#line 1234
            uint _S34 = arg_word_0(face_0, index_0, 4U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->args_0+_S34, 0U, memory_order_relaxed);

#line 1235
            uint _S35 = mesh_arg_word_0(face_0, index_0, 0U, &kernelContext_18);

#line 1242
            atomic<uint> device* _S36 = (&kernelContext_18)->counts_and_mesh_args_0+_S35;

#line 1242
            uint _S37 = bucket_clusters_0(index_0, &kernelContext_18);

#line 1242
            atomic_store_explicit(_S36, _S37, memory_order_relaxed);

#line 1242
            uint _S38 = mesh_arg_word_0(face_0, index_0, 2U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S38, 1U, memory_order_relaxed);

#line 1224
            face_0 = face_0 + 1U;

#line 1224
        }

#line 1214
    }

#line 1214
    uint _S39 = survivor_count_0(&kernelContext_18);

#line 1249
    if(index_0 >= _S39)
    {
        return;
    }



    uint device* _S40 = (&kernelContext_18)->visible_instances_0+index_0;

#line 1256
    uint entry_0 = *_S40;


    uint instance_index_0 = (*_S40) & 16777215U;

#line 1259
    MeshLevels_0 _S41 = mesh_levels_of_0(((&kernelContext_18)->instances_0+instance_index_0)->mesh_0, &kernelContext_18);

#line 1259
    uint _S42 = select_level_0(instance_index_0, instance_index_0, &kernelContext_18);

#line 1259
    uint _S43 = level_mesh_at_0(_S41.first_level_0 + _S42, &kernelContext_18);

#line 1259
    uint _S44 = instance_material_mode_0(instance_index_0, &kernelContext_18);

#line 1259
    uint _S45 = bucket_for_0(_S43, _S44, &kernelContext_18);

#line 1278
    if(_S45 != 4294967295U)
    {

#line 1287
        if(((&kernelContext_18)->gen_0->mode_0) == 2U)
        {

#line 1287
            face_0 = 0U;


            for(;;)
            {

#line 1290
                if(face_0 < 6U)
                {
                }
                else
                {

#line 1290
                    break;
                }
                if(((entry_0 >> (24U + face_0)) & 1U) != 0U)
                {

#line 1292
                    uint _S46 = mesh_arg_word_0(1U + face_0, _S45, 1U, &kernelContext_18);


                    uint _S47 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S46, 1U, memory_order_relaxed);

#line 1292
                }

#line 1290
                face_0 = face_0 + 1U;

#line 1290
            }

#line 1287
        }
        else
        {

#line 1287
            uint _S48 = mesh_arg_word_0(0U, _S45, 1U, &kernelContext_18);

#line 1304
            uint _S49 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S48, 1U, memory_order_relaxed);

#line 1304
            bool _S50;
            if(((&kernelContext_18)->gen_0->mode_0) == 1U)
            {

#line 1305
                _S50 = (entry_0 & 2147483648U) == 0U;

#line 1305
            }
            else
            {

#line 1305
                _S50 = false;

#line 1305
            }

#line 1305
            if(_S50)
            {

#line 1305
                uint _S51 = mesh_arg_word_0(1U, _S45, 1U, &kernelContext_18);


                uint _S52 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S51, 1U, memory_order_relaxed);

#line 1305
            }

#line 1287
        }

#line 1278
    }

#line 1278
    uint _S53 = route_word_0(index_0, &kernelContext_18);

#line 1315
    *((&kernelContext_18)->visible_instances_0+_S53) = _S45;
    return;
}


#line 1340
uint starts_slot_count_0(KernelContext_0 thread* kernelContext_19)
{

#line 1340
    uint _S54;

    if((kernelContext_19->gen_0->mode_0) == 2U)
    {

#line 1342
        _S54 = 6U * kernelContext_19->gen_0->bucket_count_0;

#line 1342
    }
    else
    {

#line 1342
        _S54 = kernelContext_19->gen_0->bucket_count_0;

#line 1342
    }

#line 1342
    return _S54;
}


uint starts_slot_region_0(uint slot_1, KernelContext_0 thread* kernelContext_20)
{

#line 1346
    uint _S55;

    if((kernelContext_20->gen_0->mode_0) == 2U)
    {

#line 1348
        uint _S56 = slot_1 / kernelContext_20->gen_0->bucket_count_0;

#line 1348
        _S55 = 1U + _S56;

#line 1348
    }
    else
    {

#line 1348
        _S55 = 0U;

#line 1348
    }

#line 1348
    return _S55;
}


uint starts_slot_bucket_0(uint slot_2, KernelContext_0 thread* kernelContext_21)
{

#line 1352
    uint _S57;

    if((kernelContext_21->gen_0->mode_0) == 2U)
    {

#line 1354
        uint _S58 = slot_2 % kernelContext_21->gen_0->bucket_count_0;

#line 1354
        _S57 = _S58;

#line 1354
    }
    else
    {

#line 1354
        _S57 = slot_2;

#line 1354
    }

#line 1354
    return _S57;
}



uint starts_slot_runs_0(uint slot_3, KernelContext_0 thread* kernelContext_22)
{

#line 1359
    uint _S59 = starts_slot_region_0(slot_3, kernelContext_22);

#line 1359
    uint _S60 = starts_slot_bucket_0(slot_3, kernelContext_22);

#line 1359
    uint _S61 = mesh_arg_word_0(_S59, _S60, 1U, kernelContext_22);


    uint _S62 = atomic_load_explicit(kernelContext_22->counts_and_mesh_args_0+_S61, memory_order_relaxed);

#line 1361
    return _S62;
}


#line 1596
uint starts_slot_chunk_instances_0(uint slot_4, KernelContext_0 thread* kernelContext_23)
{
    if((kernelContext_23->gen_0->mode_0) == 1U)
    {

#line 1598
        uint _S63 = starts_slot_bucket_0(slot_4, kernelContext_23);

#line 1598
        uint _S64 = mesh_arg_word_0(1U, _S63, 1U, kernelContext_23);


        uint _S65 = atomic_load_explicit(kernelContext_23->counts_and_mesh_args_0+_S64, memory_order_relaxed);

#line 1600
        return _S65;
    }

#line 1600
    uint _S66 = starts_slot_runs_0(slot_4, kernelContext_23);


    return _S66;
}


#line 1398
uint task_chunks_0(uint bucket_9, uint instances_2, KernelContext_0 thread* kernelContext_24)
{
    uint lanes_0 = kernelContext_24->gen_0->task_lanes_0;
    if((kernelContext_24->gen_0->task_lanes_0) == 0U)
    {
        return 0U;
    }

#line 1403
    uint _S67 = bucket_clusters_0(bucket_9, kernelContext_24);


    uint _S68 = _S67 / lanes_0;

#line 1406
    uint _S69 = _S68 * instances_2;

#line 1406
    uint _S70 = _S67 % lanes_0;

#line 1406
    uint _S71 = (_S70 * instances_2 + lanes_0 - 1U) / lanes_0;

#line 1406
    return _S69 + _S71;
}


#line 942
uint runs_at_0(KernelContext_0 thread* kernelContext_25)
{
    return 2U * kernelContext_25->gen_0->visible_capacity_0;
}


#line 1426
uint workgroup_exclusive_scan_0(uint lane_0, uint value_0, KernelContext_0 thread* kernelContext_26)
{
    threadgroup_barrier(mem_flags::mem_threadgroup);
    (*kernelContext_26->starts_chunk_runs_0)[lane_0] = value_0;
    threadgroup_barrier(mem_flags::mem_threadgroup);

#line 1430
    uint reach_0 = 1U;
    for(;;)
    {

#line 1431
        if(reach_0 < 256U)
        {
        }
        else
        {

#line 1431
            break;
        }

#line 1431
        uint behind_0;

        if(lane_0 >= reach_0)
        {

#line 1433
            behind_0 = (*kernelContext_26->starts_chunk_runs_0)[lane_0 - reach_0];

#line 1433
        }
        else
        {

#line 1433
            behind_0 = 0U;

#line 1433
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
        (*kernelContext_26->starts_chunk_runs_0)[lane_0] = (*kernelContext_26->starts_chunk_runs_0)[lane_0] + behind_0;
        threadgroup_barrier(mem_flags::mem_threadgroup);

#line 1431
        reach_0 = reach_0 << 1U;

#line 1431
    }

#line 1438
    return (*kernelContext_26->starts_chunk_runs_0)[lane_0] - value_0;
}


#line 1019
uint chunk_start_word_0(uint region_6, uint bucket_10, KernelContext_0 thread* kernelContext_27)
{

#line 1019
    uint _S72 = bucket_mesh_word_0(kernelContext_27->gen_0->bucket_count_0, kernelContext_27);

    return _S72 + region_6 * (kernelContext_27->gen_0->bucket_count_0 + 1U) + bucket_10;
}


#line 1444
void write_chunk_start_0(uint region_7, uint bucket_11, uint start_0, uint chunks_0, KernelContext_0 thread* kernelContext_28)
{
    if((kernelContext_28->gen_0->task_lanes_0) == 0U)
    {
        return;
    }

#line 1448
    uint _S73 = chunk_start_word_0(region_7, bucket_11, kernelContext_28);

    *(kernelContext_28->visible_instances_0+_S73) = start_0;
    if((bucket_11 + 1U) == (kernelContext_28->gen_0->bucket_count_0))
    {

#line 1451
        uint _S74 = chunk_start_word_0(region_7, kernelContext_28->gen_0->bucket_count_0, kernelContext_28);

        *(kernelContext_28->visible_instances_0+_S74) = start_0 + chunks_0;

#line 1451
    }



    return;
}


#line 984
uint count_word_0(uint region_8, uint bucket_12, KernelContext_0 thread* kernelContext_29)
{
    return region_8 * 4U * kernelContext_29->gen_0->bucket_count_0 + bucket_12;
}


#line 1412
void store_task_dispatch_0(uint at_3, uint chunks_1, KernelContext_0 thread* kernelContext_30)
{
    atomic_store_explicit(kernelContext_30->counts_and_mesh_args_0+at_3, min(chunks_1, 65535U), memory_order_relaxed);

    atomic_store_explicit(kernelContext_30->counts_and_mesh_args_0+(at_3 + 1U), (chunks_1 + 65535U - 1U) / 65535U, memory_order_relaxed);
    atomic_store_explicit(kernelContext_30->counts_and_mesh_args_0+(at_3 + 2U), 1U, memory_order_relaxed);
    return;
}


#line 1385
void write_task_extents_0(uint region_9, uint bucket_13, uint instances_3, KernelContext_0 thread* kernelContext_31)
{
    if((kernelContext_31->gen_0->task_lanes_0) == 0U)
    {
        return;
    }

#line 1389
    uint _S75 = mesh_arg_word_0(region_9, bucket_13, 0U, kernelContext_31);

#line 1389
    uint _S76 = task_chunks_0(bucket_13, instances_3, kernelContext_31);

#line 1389
    store_task_dispatch_0(_S75, _S76, kernelContext_31);


    return;
}


#line 1028
uint flat_arg_word_0(uint region_10, uint segment_0, uint slot_5, KernelContext_0 thread* kernelContext_32)
{
    return kernelContext_32->gen_0->draw_regions_0 * 4U * kernelContext_32->gen_0->bucket_count_0 + (region_10 * 6U + segment_0) * 3U + slot_5;
}


#line 1469
void write_flat_args_0(uint lane_1, uint first_region_0, uint count_0, uint stride_0, KernelContext_0 thread* kernelContext_33)
{
    threadgroup_barrier(mem_flags::mem_device | mem_flags::mem_threadgroup | mem_flags::mem_texture | mem_flags::mem_threadgroup_imageblock);

#line 1471
    bool _S77;
    if((kernelContext_33->gen_0->task_lanes_0) == 0U)
    {

#line 1472
        _S77 = true;

#line 1472
    }
    else
    {

#line 1472
        _S77 = lane_1 >= (count_0 * 6U);

#line 1472
    }

#line 1472
    if(_S77)
    {
        return;
    }
    uint region_11 = first_region_0 + lane_1 / 6U * stride_0;
    uint segment_1 = lane_1 % 6U;
    uint _S78 = segment_1 * 2U;

#line 1478
    uint first_0 = kernelContext_33->tables_0[kernelContext_33->gen_0->flat_segments_at_0 + _S78];
    uint end_0 = kernelContext_33->tables_0[kernelContext_33->gen_0->flat_segments_at_0 + _S78 + 1U];

#line 1479
    uint chunks_2;

    if(first_0 < end_0)
    {

#line 1481
        uint _S79 = chunk_start_word_0(region_11, end_0, kernelContext_33);
        uint _S80 = *(kernelContext_33->visible_instances_0+_S79);

#line 1482
        uint _S81 = chunk_start_word_0(region_11, first_0, kernelContext_33);

#line 1482
        chunks_2 = _S80 - *(kernelContext_33->visible_instances_0+_S81);

#line 1481
    }
    else
    {

#line 1481
        chunks_2 = 0U;

#line 1481
    }

#line 1481
    uint _S82 = flat_arg_word_0(region_11, segment_1, 0U, kernelContext_33);

#line 1481
    store_task_dispatch_0(_S82, chunks_2, kernelContext_33);



    return;
}


#line 1517
[[kernel]] void startsMain(uint3 thread_1 [[thread_position_in_threadgroup]], DrawGenParams_0 constant* gen_2 [[buffer(0)]], uint device* tables_2 [[buffer(4)]], GpuMesh_0 device* meshes_2 [[buffer(2)]], uint device* visible_instances_2 [[buffer(5)]], atomic<uint> device* args_2 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_2 [[buffer(7)]], uint device* visible_count_2 [[buffer(3)]], GpuInstance_natural_0 device* instances_4 [[buffer(1)]], uint device* group_state_2 [[buffer(8)]])
{

#line 1517
    thread KernelContext_0 kernelContext_34;

#line 1517
    (&kernelContext_34)->gen_0 = gen_2;

#line 1517
    (&kernelContext_34)->tables_0 = tables_2;

#line 1517
    (&kernelContext_34)->meshes_0 = meshes_2;

#line 1517
    (&kernelContext_34)->visible_instances_0 = visible_instances_2;

#line 1517
    (&kernelContext_34)->args_0 = args_2;

#line 1517
    (&kernelContext_34)->counts_and_mesh_args_0 = counts_and_mesh_args_2;

#line 1517
    (&kernelContext_34)->visible_count_0 = visible_count_2;

#line 1517
    (&kernelContext_34)->instances_0 = instances_4;

#line 1517
    (&kernelContext_34)->group_state_0 = group_state_2;

#line 1517
    threadgroup array<uint, int(256)> starts_chunk_runs_2;

#line 1517
    (&kernelContext_34)->starts_chunk_runs_0 = &starts_chunk_runs_2;

    uint lane_2 = thread_1.x;

#line 1519
    uint _S83 = starts_slot_count_0(&kernelContext_34);

    uint chunk_0 = (_S83 + 256U - 1U) / 256U;
    uint _S84 = min(lane_2 * chunk_0, _S83);
    uint _S85 = min(_S84 + chunk_0, _S83);

#line 1523
    uint slot_6 = _S84;

#line 1523
    uint total_0 = 0U;

#line 1523
    uint tasks_0 = 0U;

#line 1531
    for(;;)
    {

#line 1531
        if(slot_6 < _S85)
        {
        }
        else
        {

#line 1531
            break;
        }

#line 1531
        uint _S86 = starts_slot_runs_0(slot_6, &kernelContext_34);

        uint total_1 = total_0 + _S86;

#line 1533
        uint _S87 = starts_slot_bucket_0(slot_6, &kernelContext_34);

#line 1533
        uint _S88 = starts_slot_chunk_instances_0(slot_6, &kernelContext_34);

#line 1533
        uint _S89 = task_chunks_0(_S87, _S88, &kernelContext_34);
        uint tasks_1 = tasks_0 + _S89;

#line 1531
        slot_6 = slot_6 + 1U;

#line 1531
        total_0 = total_1;

#line 1531
        tasks_0 = tasks_1;

#line 1531
    }

#line 1540
    if(((&kernelContext_34)->gen_0->mode_0) == 2U)
    {

#line 1540
        slot_6 = (&kernelContext_34)->gen_0->face_runs_at_0;

#line 1540
    }
    else
    {

#line 1540
        uint _S90 = runs_at_0(&kernelContext_34);

#line 1540
        slot_6 = _S90;

#line 1540
    }

#line 1540
    uint _S91 = workgroup_exclusive_scan_0(lane_2, total_0, &kernelContext_34);
    uint _S92 = slot_6 + _S91;

#line 1541
    uint _S93 = workgroup_exclusive_scan_0(lane_2, tasks_0, &kernelContext_34);

#line 1541
    slot_6 = _S84;

#line 1541
    uint task_start_0 = _S93;

#line 1541
    uint start_1 = _S92;

    for(;;)
    {

#line 1543
        if(slot_6 < _S85)
        {
        }
        else
        {

#line 1543
            break;
        }

#line 1543
        uint _S94 = starts_slot_region_0(slot_6, &kernelContext_34);

#line 1543
        uint _S95 = starts_slot_bucket_0(slot_6, &kernelContext_34);

#line 1543
        uint _S96 = starts_slot_runs_0(slot_6, &kernelContext_34);

#line 1543
        uint _S97 = starts_slot_chunk_instances_0(slot_6, &kernelContext_34);

#line 1543
        uint _S98 = task_chunks_0(_S95, _S97, &kernelContext_34);

#line 1552
        if(((&kernelContext_34)->gen_0->mode_0) == 1U)
        {

#line 1552
            total_0 = 1U;

#line 1552
        }
        else
        {

#line 1552
            total_0 = _S94;

#line 1552
        }

#line 1552
        write_chunk_start_0(total_0, _S95, task_start_0, _S98, &kernelContext_34);
        uint task_start_1 = task_start_0 + _S98;

#line 1553
        uint _S99 = run_start_word_0(_S94, _S95, &kernelContext_34);
        *((&kernelContext_34)->visible_instances_0+_S99) = start_1;


        if(_S96 != 0U)
        {

#line 1557
            uint _S100 = count_word_0(_S94, _S95, &kernelContext_34);

            atomic_store_explicit((&kernelContext_34)->counts_and_mesh_args_0+_S100, 1U, memory_order_relaxed);

#line 1557
        }

#line 1557
        write_task_extents_0(_S94, _S95, _S96, &kernelContext_34);

#line 1565
        if(((&kernelContext_34)->gen_0->mode_0) == 1U)
        {

#line 1565
            uint _S101 = mesh_arg_word_0(1U, _S95, 1U, &kernelContext_34);

#line 1571
            uint early_0 = atomic_load_explicit((&kernelContext_34)->counts_and_mesh_args_0+_S101, memory_order_relaxed);

#line 1571
            uint _S102 = run_start_word_0(1U, _S95, &kernelContext_34);
            *((&kernelContext_34)->visible_instances_0+_S102) = start_1;

#line 1572
            uint _S103 = run_start_word_0(2U, _S95, &kernelContext_34);
            *((&kernelContext_34)->visible_instances_0+_S103) = start_1 + early_0;
            if(early_0 != 0U)
            {

#line 1574
                uint _S104 = count_word_0(1U, _S95, &kernelContext_34);

                atomic_store_explicit((&kernelContext_34)->counts_and_mesh_args_0+_S104, 1U, memory_order_relaxed);

#line 1574
            }

#line 1574
            write_task_extents_0(1U, _S95, early_0, &kernelContext_34);

#line 1565
        }

#line 1580
        uint start_2 = start_1 + _S96;

#line 1543
        slot_6 = slot_6 + 1U;

#line 1543
        task_start_0 = task_start_1;

#line 1543
        start_1 = start_2;

#line 1543
    }

#line 1586
    bool faces_0 = ((&kernelContext_34)->gen_0->mode_0) == 2U;

    if(faces_0)
    {

#line 1588
        slot_6 = 1U;

#line 1588
    }
    else
    {

#line 1588
        if(((&kernelContext_34)->gen_0->mode_0) == 1U)
        {

#line 1588
            slot_6 = 1U;

#line 1588
        }
        else
        {

#line 1588
            slot_6 = 0U;

#line 1588
        }

#line 1588
    }
    if(faces_0)
    {

#line 1589
        total_0 = 6U;

#line 1589
    }
    else
    {

#line 1589
        total_0 = 1U;

#line 1589
    }

#line 1589
    write_flat_args_0(lane_2, slot_6, total_0, 1U, &kernelContext_34);

    return;
}


#line 1615
[[kernel]] void scatterMain(uint3 thread_2 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_3 [[buffer(0)]], uint device* tables_3 [[buffer(4)]], GpuMesh_0 device* meshes_3 [[buffer(2)]], uint device* visible_instances_3 [[buffer(5)]], atomic<uint> device* args_3 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_3 [[buffer(7)]], uint device* visible_count_3 [[buffer(3)]], GpuInstance_natural_0 device* instances_5 [[buffer(1)]], uint device* group_state_3 [[buffer(8)]])
{

#line 1615
    thread KernelContext_0 kernelContext_35;

#line 1615
    (&kernelContext_35)->gen_0 = gen_3;

#line 1615
    (&kernelContext_35)->tables_0 = tables_3;

#line 1615
    (&kernelContext_35)->meshes_0 = meshes_3;

#line 1615
    (&kernelContext_35)->visible_instances_0 = visible_instances_3;

#line 1615
    (&kernelContext_35)->args_0 = args_3;

#line 1615
    (&kernelContext_35)->counts_and_mesh_args_0 = counts_and_mesh_args_3;

#line 1615
    (&kernelContext_35)->visible_count_0 = visible_count_3;

#line 1615
    (&kernelContext_35)->instances_0 = instances_5;

#line 1615
    (&kernelContext_35)->group_state_0 = group_state_3;

#line 1615
    threadgroup array<uint, int(256)> starts_chunk_runs_3;

#line 1615
    (&kernelContext_35)->starts_chunk_runs_0 = &starts_chunk_runs_3;

    uint index_1 = thread_2.x;

#line 1617
    uint _S105 = survivor_count_0(&kernelContext_35);
    if(index_1 >= _S105)
    {
        return;
    }

#line 1620
    uint _S106 = route_word_0(index_1, &kernelContext_35);

    uint device* _S107 = (&kernelContext_35)->visible_instances_0+_S106;

#line 1622
    uint bucket_14 = *_S107;
    if((*_S107) == 4294967295U)
    {
        return;
    }
    uint device* _S108 = (&kernelContext_35)->visible_instances_0+index_1;

#line 1627
    uint entry_1 = *_S108;
    uint instance_index_1 = (*_S108) & 16777215U;

#line 1628
    uint face_1;
    if(((&kernelContext_35)->gen_0->mode_0) == 2U)
    {

#line 1629
        face_1 = 0U;

        for(;;)
        {

#line 1631
            if(face_1 < 6U)
            {
            }
            else
            {

#line 1631
                break;
            }
            if(((entry_1 >> (24U + face_1)) & 1U) == 0U)
            {
                face_1 = face_1 + 1U;

#line 1631
                continue;
            }

#line 1637
            uint region_12 = 1U + face_1;

#line 1637
            uint _S109 = arg_word_0(region_12, bucket_14, 1U, &kernelContext_35);
            uint face_slot_0 = atomic_fetch_add_explicit((&kernelContext_35)->args_0+_S109, 1U, memory_order_relaxed);

#line 1638
            uint _S110 = run_start_word_0(region_12, bucket_14, &kernelContext_35);
            *((&kernelContext_35)->visible_instances_0+(*((&kernelContext_35)->visible_instances_0+_S110) + face_slot_0)) = instance_index_1;

#line 1631
            face_1 = face_1 + 1U;

#line 1631
        }

#line 1642
        return;
    }

#line 1642
    bool _S111;


    if(((&kernelContext_35)->gen_0->mode_0) == 1U)
    {

#line 1645
        _S111 = (entry_1 & 2147483648U) != 0U;

#line 1645
    }
    else
    {

#line 1645
        _S111 = false;

#line 1645
    }

#line 1645
    if(_S111)
    {
        return;
    }
    if(((&kernelContext_35)->gen_0->mode_0) == 1U)
    {

#line 1649
        face_1 = 1U;

#line 1649
    }
    else
    {

#line 1649
        face_1 = 0U;

#line 1649
    }

#line 1649
    uint _S112 = arg_word_0(face_1, bucket_14, 1U, &kernelContext_35);
    uint slot_7 = atomic_fetch_add_explicit((&kernelContext_35)->args_0+_S112, 1U, memory_order_relaxed);

#line 1650
    uint _S113 = run_start_word_0(face_1, bucket_14, &kernelContext_35);
    *((&kernelContext_35)->visible_instances_0+(*((&kernelContext_35)->visible_instances_0+_S113) + slot_7)) = instance_index_1;
    return;
}


#line 1663
[[kernel]] void lateScatterMain(uint3 thread_3 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_4 [[buffer(0)]], uint device* tables_4 [[buffer(4)]], GpuMesh_0 device* meshes_4 [[buffer(2)]], uint device* visible_instances_4 [[buffer(5)]], atomic<uint> device* args_4 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_4 [[buffer(7)]], uint device* visible_count_4 [[buffer(3)]], GpuInstance_natural_0 device* instances_6 [[buffer(1)]], uint device* group_state_4 [[buffer(8)]])
{

#line 1663
    thread KernelContext_0 kernelContext_36;

#line 1663
    (&kernelContext_36)->gen_0 = gen_4;

#line 1663
    (&kernelContext_36)->tables_0 = tables_4;

#line 1663
    (&kernelContext_36)->meshes_0 = meshes_4;

#line 1663
    (&kernelContext_36)->visible_instances_0 = visible_instances_4;

#line 1663
    (&kernelContext_36)->args_0 = args_4;

#line 1663
    (&kernelContext_36)->counts_and_mesh_args_0 = counts_and_mesh_args_4;

#line 1663
    (&kernelContext_36)->visible_count_0 = visible_count_4;

#line 1663
    (&kernelContext_36)->instances_0 = instances_6;

#line 1663
    (&kernelContext_36)->group_state_0 = group_state_4;

#line 1663
    threadgroup array<uint, int(256)> starts_chunk_runs_4;

#line 1663
    (&kernelContext_36)->starts_chunk_runs_0 = &starts_chunk_runs_4;

    uint index_2 = thread_3.x;

#line 1665
    uint _S114 = survivor_count_0(&kernelContext_36);
    if(index_2 >= _S114)
    {
        return;
    }
    uint device* _S115 = (&kernelContext_36)->visible_instances_0+index_2;

#line 1670
    uint entry_2 = *_S115;
    if(((*_S115) & 1073741824U) == 0U)
    {
        return;
    }

#line 1673
    uint _S116 = route_word_0(index_2, &kernelContext_36);

    uint device* _S117 = (&kernelContext_36)->visible_instances_0+_S116;

#line 1675
    uint bucket_15 = *_S117;
    if((*_S117) == 4294967295U)
    {
        return;
    }

#line 1678
    uint _S118 = arg_word_0(2U, bucket_15, 1U, &kernelContext_36);

    uint slot_8 = atomic_fetch_add_explicit((&kernelContext_36)->args_0+_S118, 1U, memory_order_relaxed);

#line 1680
    uint _S119 = run_start_word_0(2U, bucket_15, &kernelContext_36);
    *((&kernelContext_36)->visible_instances_0+(*((&kernelContext_36)->visible_instances_0+_S119) + slot_8)) = entry_2 & 16777215U;

    return;
}


#line 1700
[[kernel]] void lateFinishMain(uint3 thread_4 [[thread_position_in_threadgroup]], DrawGenParams_0 constant* gen_5 [[buffer(0)]], uint device* tables_5 [[buffer(4)]], GpuMesh_0 device* meshes_5 [[buffer(2)]], uint device* visible_instances_5 [[buffer(5)]], atomic<uint> device* args_5 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_5 [[buffer(7)]], uint device* visible_count_5 [[buffer(3)]], GpuInstance_natural_0 device* instances_7 [[buffer(1)]], uint device* group_state_5 [[buffer(8)]])
{

#line 1700
    thread KernelContext_0 kernelContext_37;

#line 1700
    (&kernelContext_37)->gen_0 = gen_5;

#line 1700
    (&kernelContext_37)->tables_0 = tables_5;

#line 1700
    (&kernelContext_37)->meshes_0 = meshes_5;

#line 1700
    (&kernelContext_37)->visible_instances_0 = visible_instances_5;

#line 1700
    (&kernelContext_37)->args_0 = args_5;

#line 1700
    (&kernelContext_37)->counts_and_mesh_args_0 = counts_and_mesh_args_5;

#line 1700
    (&kernelContext_37)->visible_count_0 = visible_count_5;

#line 1700
    (&kernelContext_37)->instances_0 = instances_7;

#line 1700
    (&kernelContext_37)->group_state_0 = group_state_5;

#line 1700
    threadgroup array<uint, int(256)> starts_chunk_runs_5;

#line 1700
    (&kernelContext_37)->starts_chunk_runs_0 = &starts_chunk_runs_5;

    uint lane_3 = thread_4.x;
    uint chunk_1 = (gen_5->bucket_count_0 + 256U - 1U) / 256U;
    uint _S120 = min(lane_3 * chunk_1, gen_5->bucket_count_0);
    uint _S121 = min(_S120 + chunk_1, gen_5->bucket_count_0);

#line 1705
    uint bucket_16 = _S120;

#line 1705
    uint drawn_tasks_0 = 0U;

#line 1705
    uint late_tasks_0 = 0U;



    for(;;)
    {

#line 1709
        if(bucket_16 < _S121)
        {
        }
        else
        {

#line 1709
            break;
        }

#line 1709
        uint _S122 = arg_word_0(1U, bucket_16, 1U, &kernelContext_37);

        uint early_1 = atomic_load_explicit((&kernelContext_37)->args_0+_S122, memory_order_relaxed);

#line 1711
        uint _S123 = arg_word_0(2U, bucket_16, 1U, &kernelContext_37);
        uint late_0 = atomic_load_explicit((&kernelContext_37)->args_0+_S123, memory_order_relaxed);

#line 1712
        uint _S124 = mesh_arg_word_0(2U, bucket_16, 1U, &kernelContext_37);
        atomic_store_explicit((&kernelContext_37)->counts_and_mesh_args_0+_S124, late_0, memory_order_relaxed);
        if(late_0 != 0U)
        {

#line 1714
            uint _S125 = count_word_0(2U, bucket_16, &kernelContext_37);

            atomic_store_explicit((&kernelContext_37)->counts_and_mesh_args_0+_S125, 1U, memory_order_relaxed);

#line 1714
        }



        uint drawn_0 = early_1 + late_0;

#line 1718
        uint _S126 = arg_word_0(0U, bucket_16, 1U, &kernelContext_37);
        atomic_store_explicit((&kernelContext_37)->args_0+_S126, drawn_0, memory_order_relaxed);

#line 1719
        uint _S127 = mesh_arg_word_0(0U, bucket_16, 1U, &kernelContext_37);
        atomic_store_explicit((&kernelContext_37)->counts_and_mesh_args_0+_S127, drawn_0, memory_order_relaxed);

#line 1720
        uint _S128 = count_word_0(0U, bucket_16, &kernelContext_37);
        atomic<uint> device* _S129 = (&kernelContext_37)->counts_and_mesh_args_0+_S128;

#line 1721
        int _S130;

#line 1721
        if(drawn_0 != 0U)
        {

#line 1721
            _S130 = int(1);

#line 1721
        }
        else
        {

#line 1721
            _S130 = int(0);

#line 1721
        }

#line 1721
        atomic_store_explicit(_S129, uint(_S130), memory_order_relaxed);

#line 1721
        write_task_extents_0(2U, bucket_16, late_0, &kernelContext_37);

#line 1721
        write_task_extents_0(0U, bucket_16, drawn_0, &kernelContext_37);

#line 1721
        uint _S131 = task_chunks_0(bucket_16, drawn_0, &kernelContext_37);

#line 1726
        uint drawn_tasks_1 = drawn_tasks_0 + _S131;

#line 1726
        uint _S132 = task_chunks_0(bucket_16, late_0, &kernelContext_37);
        uint late_tasks_1 = late_tasks_0 + _S132;

#line 1709
        bucket_16 = bucket_16 + 1U;

#line 1709
        drawn_tasks_0 = drawn_tasks_1;

#line 1709
        late_tasks_0 = late_tasks_1;

#line 1709
    }

#line 1709
    uint _S133 = workgroup_exclusive_scan_0(lane_3, drawn_tasks_0, &kernelContext_37);

#line 1709
    uint _S134 = workgroup_exclusive_scan_0(lane_3, late_tasks_0, &kernelContext_37);

#line 1709
    bucket_16 = _S120;

#line 1709
    uint drawn_start_0 = _S133;

#line 1709
    uint late_start_0 = _S134;

#line 1734
    for(;;)
    {

#line 1734
        if(bucket_16 < _S121)
        {
        }
        else
        {

#line 1734
            break;
        }

#line 1734
        uint _S135 = arg_word_0(1U, bucket_16, 1U, &kernelContext_37);

        uint early_2 = atomic_load_explicit((&kernelContext_37)->args_0+_S135, memory_order_relaxed);

#line 1736
        uint _S136 = arg_word_0(2U, bucket_16, 1U, &kernelContext_37);
        uint late_1 = atomic_load_explicit((&kernelContext_37)->args_0+_S136, memory_order_relaxed);

#line 1737
        uint _S137 = task_chunks_0(bucket_16, early_2 + late_1, &kernelContext_37);

#line 1737
        uint _S138 = task_chunks_0(bucket_16, late_1, &kernelContext_37);

#line 1737
        write_chunk_start_0(0U, bucket_16, drawn_start_0, _S137, &kernelContext_37);

#line 1737
        write_chunk_start_0(2U, bucket_16, late_start_0, _S138, &kernelContext_37);

#line 1742
        uint drawn_start_1 = drawn_start_0 + _S137;
        uint late_start_1 = late_start_0 + _S138;

#line 1734
        bucket_16 = bucket_16 + 1U;

#line 1734
        drawn_start_0 = drawn_start_1;

#line 1734
        late_start_0 = late_start_1;

#line 1734
    }

#line 1734
    write_flat_args_0(lane_3, 0U, 2U, 2U, &kernelContext_37);

#line 1746
    return;
}

