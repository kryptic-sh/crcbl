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
    uint bucket_lookup_at_0;
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


#line 631
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 631
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


#line 1340
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


#line 794
uint bucket_mesh_0(uint bucket_0, KernelContext_0 thread* kernelContext_0)
{
    return kernelContext_0->tables_0[bucket_0];
}


#line 916
uint draw_slot_0(uint region_0, uint bucket_1, KernelContext_0 thread* kernelContext_1)
{
    return region_0 * kernelContext_1->gen_0->bucket_count_0 + bucket_1;
}


#line 916
uint draw_slot_1(uint region_1, uint bucket_2, KernelContext_0 thread* kernelContext_2)
{
    return region_1 * kernelContext_2->gen_0->bucket_count_0 + bucket_2;
}


#line 927
uint run_start_word_0(uint region_2, uint bucket_3, KernelContext_0 thread* kernelContext_3)
{
    uint _S1 = 3U * kernelContext_3->gen_0->visible_capacity_0;

#line 929
    uint _S2 = draw_slot_0(region_2, bucket_3, kernelContext_3);

#line 929
    return _S1 + _S2;
}


#line 927
uint run_start_word_1(uint region_3, uint bucket_4, KernelContext_0 thread* kernelContext_4)
{
    uint _S3 = 3U * kernelContext_4->gen_0->visible_capacity_0;

#line 929
    uint _S4 = draw_slot_0(region_3, bucket_4, kernelContext_4);

#line 929
    return _S3 + _S4;
}


#line 941
uint bucket_mesh_word_0(uint bucket_5, KernelContext_0 thread* kernelContext_5)
{

#line 941
    uint _S5 = run_start_word_1(7U, bucket_5, kernelContext_5);

    return _S5;
}


#line 963
uint arg_word_0(uint region_4, uint bucket_6, uint field_0, KernelContext_0 thread* kernelContext_6)
{

#line 963
    uint _S6 = draw_slot_1(region_4, bucket_6, kernelContext_6);

    return _S6 * 5U + field_0;
}


#line 956
uint mesh_arg_word_0(uint region_5, uint bucket_7, uint slot_0, KernelContext_0 thread* kernelContext_7)
{
    return region_5 * 4U * kernelContext_7->gen_0->bucket_count_0 + kernelContext_7->gen_0->bucket_count_0 + bucket_7 * 3U + slot_0;
}


#line 816
uint bucket_clusters_0(uint bucket_8, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->tables_0[kernelContext_8->gen_0->bucket_clusters_at_0 + bucket_8];
}


#line 1127
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


#line 823
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


#line 1034
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S7 = (((basis_0) * (transpose(basis_0))));

#line 1036
    float bound_0 = 0.0f;

#line 1036
    uint row_0 = 0U;

    for(;;)
    {

#line 1038
        if(row_0 < 3U)
        {
        }
        else
        {

#line 1038
            break;
        }
        float _S8 = max(bound_0, abs(_S7[row_0][int(0)]) + abs(_S7[row_0][int(1)]) + abs(_S7[row_0][int(2)]));

#line 1038
        uint row_1 = row_0 + 1U;

#line 1038
        bound_0 = _S8;

#line 1038
        row_0 = row_1;

#line 1038
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


#line 841
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


#line 1000
float projected_error_0(float error_1, float3 center_0, float radius_1, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S9 = delta_0.x;

#line 1003
    float _S10 = delta_0.y;

#line 1003
    float _S11 = delta_0.z;
    float distance_0 = sqrt(_S9 * _S9 + _S10 * _S10 + _S11 * _S11) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 1054
uint group_is_expanded_0(float error_2, float3 center_1, float radius_2, float3 eye_1, uint was_0, KernelContext_0 thread* kernelContext_12)
{
    float projected_0 = projected_error_0(error_2, center_1, radius_2, eye_1, kernelContext_12->gen_0->lod_params_0.x);

#line 1056
    bool expanded_0;

    if(projected_0 > (kernelContext_12->gen_0->lod_params_0.y))
    {

#line 1058
        expanded_0 = true;

#line 1058
    }
    else
    {

#line 1058
        if(was_0 != 0U)
        {

#line 1058
            expanded_0 = projected_0 > (kernelContext_12->gen_0->lod_params_0.z);

#line 1058
        }
        else
        {

#line 1058
            expanded_0 = false;

#line 1058
        }

#line 1058
    }

#line 1058
    uint _S12;
    if(expanded_0)
    {

#line 1059
        _S12 = 1U;

#line 1059
    }
    else
    {

#line 1059
        _S12 = 0U;

#line 1059
    }

#line 1059
    return _S12;
}


#line 860
uint level_mesh_at_0(uint level_1, KernelContext_0 thread* kernelContext_13)
{
    return kernelContext_13->tables_0[kernelContext_13->gen_0->level_meshes_at_0 + level_1];
}


#line 889
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


#line 1142
uint select_level_0(uint _S13, uint _S14, KernelContext_0 thread* kernelContext_16)
{

#line 1142
    GpuInstance_natural_0 device* _S15 = kernelContext_16->instances_0+_S13;

#line 1142
    MeshLevels_0 _S16 = mesh_levels_of_0(_S15->mesh_0, kernelContext_16);

#line 1100
    float3 _S17 = kernelContext_16->gen_0->camera_position_0.xyz;

#line 1100
    matrix<float,int(4),int(4)>  _S18 = matrix<float,int(4),int(4)> (_S15->transform_0.data_0[int(0)][int(0)], _S15->transform_0.data_0[int(1)][int(0)], _S15->transform_0.data_0[int(2)][int(0)], _S15->transform_0.data_0[int(3)][int(0)], _S15->transform_0.data_0[int(0)][int(1)], _S15->transform_0.data_0[int(1)][int(1)], _S15->transform_0.data_0[int(2)][int(1)], _S15->transform_0.data_0[int(3)][int(1)], _S15->transform_0.data_0[int(0)][int(2)], _S15->transform_0.data_0[int(1)][int(2)], _S15->transform_0.data_0[int(2)][int(2)], _S15->transform_0.data_0[int(3)][int(2)], _S15->transform_0.data_0[int(0)][int(3)], _S15->transform_0.data_0[int(1)][int(3)], _S15->transform_0.data_0[int(2)][int(3)], _S15->transform_0.data_0[int(3)][int(3)]);
    float _S19 = max_stretch_0(matrix<float,int(3),int(3)> (_S18[int(0)].xyz, _S18[int(1)].xyz, _S18[int(2)].xyz));
    uint _S20 = _S14 * kernelContext_16->gen_0->group_stride_0;

#line 1102
    uint chosen_0 = _S16.top_level_0;

#line 1102
    uint i_0 = 0U;

    for(;;)
    {

#line 1104
        if(i_0 < (_S16.group_count_0))
        {
        }
        else
        {

#line 1104
            break;
        }
        uint at_2 = _S16.first_group_0 + i_0;

#line 1106
        LevelGroup_0 _S21 = level_group_at_0(at_2, kernelContext_16);

#line 1111
        uint _S22 = _S20 + at_2;

#line 1111
        uint _S23 = group_is_expanded_0(_S21.error_0 * _S19, (((float4(_S21.center_x_0, _S21.center_y_0, _S21.center_z_0, 1.0f)) * (_S18))).xyz, _S21.radius_0 * _S19, _S17, *(kernelContext_16->group_state_0+_S22), kernelContext_16);
        *(kernelContext_16->group_state_0+_S22) = _S23;

#line 1112
        bool _S24;
        if(_S23 == 1U)
        {

#line 1113
            _S24 = (_S21.level_0) < chosen_0;

#line 1113
        }
        else
        {

#line 1113
            _S24 = false;

#line 1113
        }

#line 1113
        if(_S24)
        {

#line 1113
            chosen_0 = _S21.level_0;

#line 1113
        }

#line 1104
        i_0 = i_0 + 1U;

#line 1104
    }

#line 1118
    return chosen_0;
}


#line 1118
uint instance_material_mode_0(uint _S25, KernelContext_0 thread* kernelContext_17)
{

#line 808
    return (((kernelContext_17->instances_0+_S25)->flags_1) & 12U) >> 2U;
}


#line 1142
[[kernel]] void binMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* tables_1 [[buffer(4)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], uint device* visible_instances_1 [[buffer(5)]], atomic<uint> device* args_1 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_1 [[buffer(7)]], uint device* visible_count_1 [[buffer(3)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], uint device* group_state_1 [[buffer(8)]])
{

#line 1142
    thread KernelContext_0 kernelContext_18;

#line 1142
    (&kernelContext_18)->gen_0 = gen_1;

#line 1142
    (&kernelContext_18)->tables_0 = tables_1;

#line 1142
    (&kernelContext_18)->meshes_0 = meshes_1;

#line 1142
    (&kernelContext_18)->visible_instances_0 = visible_instances_1;

#line 1142
    (&kernelContext_18)->args_0 = args_1;

#line 1142
    (&kernelContext_18)->counts_and_mesh_args_0 = counts_and_mesh_args_1;

#line 1142
    (&kernelContext_18)->visible_count_0 = visible_count_1;

#line 1142
    (&kernelContext_18)->instances_0 = instances_1;

#line 1142
    (&kernelContext_18)->group_state_0 = group_state_1;

#line 1142
    threadgroup array<uint, int(256)> starts_chunk_runs_1;

#line 1142
    (&kernelContext_18)->starts_chunk_runs_0 = &starts_chunk_runs_1;

    uint index_0 = thread_0.x;

#line 1144
    uint face_0;

#line 1149
    if(index_0 < (gen_1->bucket_count_0))
    {

#line 1149
        uint _S26 = bucket_mesh_0(index_0, &kernelContext_18);

        GpuMesh_0 _S27 = (&kernelContext_18)->meshes_0[_S26];

#line 1151
        uint _S28 = bucket_mesh_word_0(index_0, &kernelContext_18);

#line 1156
        uint device* _S29 = (&kernelContext_18)->visible_instances_0+_S28;

#line 1156
        uint _S30 = bucket_mesh_0(index_0, &kernelContext_18);

#line 1156
        *_S29 = _S30;

#line 1156
        face_0 = 0U;


        for(;;)
        {

#line 1159
            if(face_0 < ((&kernelContext_18)->gen_0->draw_regions_0))
            {
            }
            else
            {

#line 1159
                break;
            }

#line 1159
            uint _S31 = arg_word_0(face_0, index_0, 0U, &kernelContext_18);

            atomic_store_explicit((&kernelContext_18)->args_0+_S31, _S27.index_count_0, memory_order_relaxed);

#line 1161
            uint _S32 = arg_word_0(face_0, index_0, 2U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->args_0+_S32, _S27.base_index_0, memory_order_relaxed);

#line 1162
            uint _S33 = arg_word_0(face_0, index_0, 3U, &kernelContext_18);

#line 1169
            atomic_store_explicit((&kernelContext_18)->args_0+_S33, 0U, memory_order_relaxed);

#line 1169
            uint _S34 = arg_word_0(face_0, index_0, 4U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->args_0+_S34, 0U, memory_order_relaxed);

#line 1170
            uint _S35 = mesh_arg_word_0(face_0, index_0, 0U, &kernelContext_18);

#line 1177
            atomic<uint> device* _S36 = (&kernelContext_18)->counts_and_mesh_args_0+_S35;

#line 1177
            uint _S37 = bucket_clusters_0(index_0, &kernelContext_18);

#line 1177
            atomic_store_explicit(_S36, _S37, memory_order_relaxed);

#line 1177
            uint _S38 = mesh_arg_word_0(face_0, index_0, 2U, &kernelContext_18);
            atomic_store_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S38, 1U, memory_order_relaxed);

#line 1159
            face_0 = face_0 + 1U;

#line 1159
        }

#line 1149
    }

#line 1149
    uint _S39 = survivor_count_0(&kernelContext_18);

#line 1184
    if(index_0 >= _S39)
    {
        return;
    }



    uint device* _S40 = (&kernelContext_18)->visible_instances_0+index_0;

#line 1191
    uint entry_0 = *_S40;


    uint instance_index_0 = (*_S40) & 16777215U;

#line 1194
    MeshLevels_0 _S41 = mesh_levels_of_0(((&kernelContext_18)->instances_0+instance_index_0)->mesh_0, &kernelContext_18);

#line 1194
    uint _S42 = select_level_0(instance_index_0, instance_index_0, &kernelContext_18);

#line 1194
    uint _S43 = level_mesh_at_0(_S41.first_level_0 + _S42, &kernelContext_18);

#line 1194
    uint _S44 = instance_material_mode_0(instance_index_0, &kernelContext_18);

#line 1194
    uint _S45 = bucket_for_0(_S43, _S44, &kernelContext_18);

#line 1213
    if(_S45 != 4294967295U)
    {

#line 1222
        if(((&kernelContext_18)->gen_0->mode_0) == 2U)
        {

#line 1222
            face_0 = 0U;


            for(;;)
            {

#line 1225
                if(face_0 < 6U)
                {
                }
                else
                {

#line 1225
                    break;
                }
                if(((entry_0 >> (24U + face_0)) & 1U) != 0U)
                {

#line 1227
                    uint _S46 = mesh_arg_word_0(1U + face_0, _S45, 1U, &kernelContext_18);


                    uint _S47 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S46, 1U, memory_order_relaxed);

#line 1227
                }

#line 1225
                face_0 = face_0 + 1U;

#line 1225
            }

#line 1222
        }
        else
        {

#line 1222
            uint _S48 = mesh_arg_word_0(0U, _S45, 1U, &kernelContext_18);

#line 1239
            uint _S49 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S48, 1U, memory_order_relaxed);

#line 1239
            bool _S50;
            if(((&kernelContext_18)->gen_0->mode_0) == 1U)
            {

#line 1240
                _S50 = (entry_0 & 2147483648U) == 0U;

#line 1240
            }
            else
            {

#line 1240
                _S50 = false;

#line 1240
            }

#line 1240
            if(_S50)
            {

#line 1240
                uint _S51 = mesh_arg_word_0(1U, _S45, 1U, &kernelContext_18);


                uint _S52 = atomic_fetch_add_explicit((&kernelContext_18)->counts_and_mesh_args_0+_S51, 1U, memory_order_relaxed);

#line 1240
            }

#line 1222
        }

#line 1213
    }

#line 1213
    uint _S53 = route_word_0(index_0, &kernelContext_18);

#line 1250
    *((&kernelContext_18)->visible_instances_0+_S53) = _S45;
    return;
}


#line 1275
uint starts_slot_count_0(KernelContext_0 thread* kernelContext_19)
{

#line 1275
    uint _S54;

    if((kernelContext_19->gen_0->mode_0) == 2U)
    {

#line 1277
        _S54 = 6U * kernelContext_19->gen_0->bucket_count_0;

#line 1277
    }
    else
    {

#line 1277
        _S54 = kernelContext_19->gen_0->bucket_count_0;

#line 1277
    }

#line 1277
    return _S54;
}


uint starts_slot_region_0(uint slot_1, KernelContext_0 thread* kernelContext_20)
{

#line 1281
    uint _S55;

    if((kernelContext_20->gen_0->mode_0) == 2U)
    {

#line 1283
        uint _S56 = slot_1 / kernelContext_20->gen_0->bucket_count_0;

#line 1283
        _S55 = 1U + _S56;

#line 1283
    }
    else
    {

#line 1283
        _S55 = 0U;

#line 1283
    }

#line 1283
    return _S55;
}


uint starts_slot_bucket_0(uint slot_2, KernelContext_0 thread* kernelContext_21)
{

#line 1287
    uint _S57;

    if((kernelContext_21->gen_0->mode_0) == 2U)
    {

#line 1289
        uint _S58 = slot_2 % kernelContext_21->gen_0->bucket_count_0;

#line 1289
        _S57 = _S58;

#line 1289
    }
    else
    {

#line 1289
        _S57 = slot_2;

#line 1289
    }

#line 1289
    return _S57;
}



uint starts_slot_runs_0(uint slot_3, KernelContext_0 thread* kernelContext_22)
{

#line 1294
    uint _S59 = starts_slot_region_0(slot_3, kernelContext_22);

#line 1294
    uint _S60 = starts_slot_bucket_0(slot_3, kernelContext_22);

#line 1294
    uint _S61 = mesh_arg_word_0(_S59, _S60, 1U, kernelContext_22);


    uint _S62 = atomic_load_explicit(kernelContext_22->counts_and_mesh_args_0+_S61, memory_order_relaxed);

#line 1296
    return _S62;
}


#line 907
uint runs_at_0(KernelContext_0 thread* kernelContext_23)
{
    return 2U * kernelContext_23->gen_0->visible_capacity_0;
}


#line 949
uint count_word_0(uint region_6, uint bucket_9, KernelContext_0 thread* kernelContext_24)
{
    return region_6 * 4U * kernelContext_24->gen_0->bucket_count_0 + bucket_9;
}


#line 1323
[[kernel]] void startsMain(uint3 thread_1 [[thread_position_in_threadgroup]], DrawGenParams_0 constant* gen_2 [[buffer(0)]], uint device* tables_2 [[buffer(4)]], GpuMesh_0 device* meshes_2 [[buffer(2)]], uint device* visible_instances_2 [[buffer(5)]], atomic<uint> device* args_2 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_2 [[buffer(7)]], uint device* visible_count_2 [[buffer(3)]], GpuInstance_natural_0 device* instances_2 [[buffer(1)]], uint device* group_state_2 [[buffer(8)]])
{

#line 1323
    uint behind_0;

#line 1323
    thread KernelContext_0 kernelContext_25;

#line 1323
    (&kernelContext_25)->gen_0 = gen_2;

#line 1323
    (&kernelContext_25)->tables_0 = tables_2;

#line 1323
    (&kernelContext_25)->meshes_0 = meshes_2;

#line 1323
    (&kernelContext_25)->visible_instances_0 = visible_instances_2;

#line 1323
    (&kernelContext_25)->args_0 = args_2;

#line 1323
    (&kernelContext_25)->counts_and_mesh_args_0 = counts_and_mesh_args_2;

#line 1323
    (&kernelContext_25)->visible_count_0 = visible_count_2;

#line 1323
    (&kernelContext_25)->instances_0 = instances_2;

#line 1323
    (&kernelContext_25)->group_state_0 = group_state_2;

#line 1323
    threadgroup array<uint, int(256)> starts_chunk_runs_2;

#line 1323
    (&kernelContext_25)->starts_chunk_runs_0 = &starts_chunk_runs_2;

    uint lane_0 = thread_1.x;

#line 1325
    uint _S63 = starts_slot_count_0(&kernelContext_25);

    uint chunk_0 = (_S63 + 256U - 1U) / 256U;
    uint _S64 = min(lane_0 * chunk_0, _S63);
    uint _S65 = min(_S64 + chunk_0, _S63);

#line 1329
    uint slot_4 = _S64;

#line 1329
    uint total_0 = 0U;


    for(;;)
    {

#line 1332
        if(slot_4 < _S65)
        {
        }
        else
        {

#line 1332
            break;
        }

#line 1332
        uint _S66 = starts_slot_runs_0(slot_4, &kernelContext_25);

        uint total_1 = total_0 + _S66;

#line 1332
        slot_4 = slot_4 + 1U;

#line 1332
        total_0 = total_1;

#line 1332
    }

#line 1340
    (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0] = total_0;
    threadgroup_barrier(mem_flags::mem_threadgroup);

#line 1341
    uint reach_0 = 1U;
    for(;;)
    {

#line 1342
        if(reach_0 < 256U)
        {
        }
        else
        {

#line 1342
            break;
        }
        if(lane_0 >= reach_0)
        {

#line 1344
            behind_0 = (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0 - reach_0];

#line 1344
        }
        else
        {

#line 1344
            behind_0 = 0U;

#line 1344
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
        (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0] = (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0] + behind_0;
        threadgroup_barrier(mem_flags::mem_threadgroup);

#line 1342
        reach_0 = reach_0 << 1U;

#line 1342
    }

#line 1353
    if(((&kernelContext_25)->gen_0->mode_0) == 2U)
    {

#line 1353
        slot_4 = (&kernelContext_25)->gen_0->face_runs_at_0;

#line 1353
    }
    else
    {

#line 1353
        uint _S67 = runs_at_0(&kernelContext_25);

#line 1353
        slot_4 = _S67;

#line 1353
    }
    uint _S68 = slot_4 + (*(&kernelContext_25)->starts_chunk_runs_0)[lane_0] - total_0;

#line 1354
    slot_4 = _S64;

#line 1354
    behind_0 = _S68;
    for(;;)
    {

#line 1355
        if(slot_4 < _S65)
        {
        }
        else
        {

#line 1355
            break;
        }

#line 1355
        uint _S69 = starts_slot_region_0(slot_4, &kernelContext_25);

#line 1355
        uint _S70 = starts_slot_bucket_0(slot_4, &kernelContext_25);

#line 1355
        uint _S71 = starts_slot_runs_0(slot_4, &kernelContext_25);

#line 1355
        uint _S72 = run_start_word_0(_S69, _S70, &kernelContext_25);

#line 1360
        *((&kernelContext_25)->visible_instances_0+_S72) = behind_0;


        if(_S71 != 0U)
        {

#line 1363
            uint _S73 = count_word_0(_S69, _S70, &kernelContext_25);

            atomic_store_explicit((&kernelContext_25)->counts_and_mesh_args_0+_S73, 1U, memory_order_relaxed);

#line 1363
        }



        if(((&kernelContext_25)->gen_0->mode_0) == 1U)
        {

#line 1367
            uint _S74 = mesh_arg_word_0(1U, _S70, 1U, &kernelContext_25);

#line 1373
            uint early_0 = atomic_load_explicit((&kernelContext_25)->counts_and_mesh_args_0+_S74, memory_order_relaxed);

#line 1373
            uint _S75 = run_start_word_0(1U, _S70, &kernelContext_25);
            *((&kernelContext_25)->visible_instances_0+_S75) = behind_0;

#line 1374
            uint _S76 = run_start_word_0(2U, _S70, &kernelContext_25);
            *((&kernelContext_25)->visible_instances_0+_S76) = behind_0 + early_0;
            if(early_0 != 0U)
            {

#line 1376
                uint _S77 = count_word_0(1U, _S70, &kernelContext_25);

                atomic_store_explicit((&kernelContext_25)->counts_and_mesh_args_0+_S77, 1U, memory_order_relaxed);

#line 1376
            }

#line 1367
        }

#line 1381
        uint start_0 = behind_0 + _S71;

#line 1355
        slot_4 = slot_4 + 1U;

#line 1355
        behind_0 = start_0;

#line 1355
    }

#line 1383
    return;
}


#line 1394
[[kernel]] void scatterMain(uint3 thread_2 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_3 [[buffer(0)]], uint device* tables_3 [[buffer(4)]], GpuMesh_0 device* meshes_3 [[buffer(2)]], uint device* visible_instances_3 [[buffer(5)]], atomic<uint> device* args_3 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_3 [[buffer(7)]], uint device* visible_count_3 [[buffer(3)]], GpuInstance_natural_0 device* instances_3 [[buffer(1)]], uint device* group_state_3 [[buffer(8)]])
{

#line 1394
    thread KernelContext_0 kernelContext_26;

#line 1394
    (&kernelContext_26)->gen_0 = gen_3;

#line 1394
    (&kernelContext_26)->tables_0 = tables_3;

#line 1394
    (&kernelContext_26)->meshes_0 = meshes_3;

#line 1394
    (&kernelContext_26)->visible_instances_0 = visible_instances_3;

#line 1394
    (&kernelContext_26)->args_0 = args_3;

#line 1394
    (&kernelContext_26)->counts_and_mesh_args_0 = counts_and_mesh_args_3;

#line 1394
    (&kernelContext_26)->visible_count_0 = visible_count_3;

#line 1394
    (&kernelContext_26)->instances_0 = instances_3;

#line 1394
    (&kernelContext_26)->group_state_0 = group_state_3;

#line 1394
    threadgroup array<uint, int(256)> starts_chunk_runs_3;

#line 1394
    (&kernelContext_26)->starts_chunk_runs_0 = &starts_chunk_runs_3;

    uint index_1 = thread_2.x;

#line 1396
    uint _S78 = survivor_count_0(&kernelContext_26);
    if(index_1 >= _S78)
    {
        return;
    }

#line 1399
    uint _S79 = route_word_0(index_1, &kernelContext_26);

    uint device* _S80 = (&kernelContext_26)->visible_instances_0+_S79;

#line 1401
    uint bucket_10 = *_S80;
    if((*_S80) == 4294967295U)
    {
        return;
    }
    uint device* _S81 = (&kernelContext_26)->visible_instances_0+index_1;

#line 1406
    uint entry_1 = *_S81;
    uint instance_index_1 = (*_S81) & 16777215U;

#line 1407
    uint face_1;
    if(((&kernelContext_26)->gen_0->mode_0) == 2U)
    {

#line 1408
        face_1 = 0U;

        for(;;)
        {

#line 1410
            if(face_1 < 6U)
            {
            }
            else
            {

#line 1410
                break;
            }
            if(((entry_1 >> (24U + face_1)) & 1U) == 0U)
            {
                face_1 = face_1 + 1U;

#line 1410
                continue;
            }

#line 1416
            uint region_7 = 1U + face_1;

#line 1416
            uint _S82 = arg_word_0(region_7, bucket_10, 1U, &kernelContext_26);
            uint face_slot_0 = atomic_fetch_add_explicit((&kernelContext_26)->args_0+_S82, 1U, memory_order_relaxed);

#line 1417
            uint _S83 = run_start_word_0(region_7, bucket_10, &kernelContext_26);
            *((&kernelContext_26)->visible_instances_0+(*((&kernelContext_26)->visible_instances_0+_S83) + face_slot_0)) = instance_index_1;

#line 1410
            face_1 = face_1 + 1U;

#line 1410
        }

#line 1421
        return;
    }

#line 1421
    bool _S84;


    if(((&kernelContext_26)->gen_0->mode_0) == 1U)
    {

#line 1424
        _S84 = (entry_1 & 2147483648U) != 0U;

#line 1424
    }
    else
    {

#line 1424
        _S84 = false;

#line 1424
    }

#line 1424
    if(_S84)
    {
        return;
    }
    if(((&kernelContext_26)->gen_0->mode_0) == 1U)
    {

#line 1428
        face_1 = 1U;

#line 1428
    }
    else
    {

#line 1428
        face_1 = 0U;

#line 1428
    }

#line 1428
    uint _S85 = arg_word_0(face_1, bucket_10, 1U, &kernelContext_26);
    uint slot_5 = atomic_fetch_add_explicit((&kernelContext_26)->args_0+_S85, 1U, memory_order_relaxed);

#line 1429
    uint _S86 = run_start_word_0(face_1, bucket_10, &kernelContext_26);
    *((&kernelContext_26)->visible_instances_0+(*((&kernelContext_26)->visible_instances_0+_S86) + slot_5)) = instance_index_1;
    return;
}


#line 1442
[[kernel]] void lateScatterMain(uint3 thread_3 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_4 [[buffer(0)]], uint device* tables_4 [[buffer(4)]], GpuMesh_0 device* meshes_4 [[buffer(2)]], uint device* visible_instances_4 [[buffer(5)]], atomic<uint> device* args_4 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_4 [[buffer(7)]], uint device* visible_count_4 [[buffer(3)]], GpuInstance_natural_0 device* instances_4 [[buffer(1)]], uint device* group_state_4 [[buffer(8)]])
{

#line 1442
    thread KernelContext_0 kernelContext_27;

#line 1442
    (&kernelContext_27)->gen_0 = gen_4;

#line 1442
    (&kernelContext_27)->tables_0 = tables_4;

#line 1442
    (&kernelContext_27)->meshes_0 = meshes_4;

#line 1442
    (&kernelContext_27)->visible_instances_0 = visible_instances_4;

#line 1442
    (&kernelContext_27)->args_0 = args_4;

#line 1442
    (&kernelContext_27)->counts_and_mesh_args_0 = counts_and_mesh_args_4;

#line 1442
    (&kernelContext_27)->visible_count_0 = visible_count_4;

#line 1442
    (&kernelContext_27)->instances_0 = instances_4;

#line 1442
    (&kernelContext_27)->group_state_0 = group_state_4;

#line 1442
    threadgroup array<uint, int(256)> starts_chunk_runs_4;

#line 1442
    (&kernelContext_27)->starts_chunk_runs_0 = &starts_chunk_runs_4;

    uint index_2 = thread_3.x;

#line 1444
    uint _S87 = survivor_count_0(&kernelContext_27);
    if(index_2 >= _S87)
    {
        return;
    }
    uint device* _S88 = (&kernelContext_27)->visible_instances_0+index_2;

#line 1449
    uint entry_2 = *_S88;
    if(((*_S88) & 1073741824U) == 0U)
    {
        return;
    }

#line 1452
    uint _S89 = route_word_0(index_2, &kernelContext_27);

    uint device* _S90 = (&kernelContext_27)->visible_instances_0+_S89;

#line 1454
    uint bucket_11 = *_S90;
    if((*_S90) == 4294967295U)
    {
        return;
    }

#line 1457
    uint _S91 = arg_word_0(2U, bucket_11, 1U, &kernelContext_27);

    uint slot_6 = atomic_fetch_add_explicit((&kernelContext_27)->args_0+_S91, 1U, memory_order_relaxed);

#line 1459
    uint _S92 = run_start_word_0(2U, bucket_11, &kernelContext_27);
    *((&kernelContext_27)->visible_instances_0+(*((&kernelContext_27)->visible_instances_0+_S92) + slot_6)) = entry_2 & 16777215U;

    return;
}


#line 1473
[[kernel]] void lateFinishMain(uint3 thread_4 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_5 [[buffer(0)]], uint device* tables_5 [[buffer(4)]], GpuMesh_0 device* meshes_5 [[buffer(2)]], uint device* visible_instances_5 [[buffer(5)]], atomic<uint> device* args_5 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_5 [[buffer(7)]], uint device* visible_count_5 [[buffer(3)]], GpuInstance_natural_0 device* instances_5 [[buffer(1)]], uint device* group_state_5 [[buffer(8)]])
{

#line 1473
    thread KernelContext_0 kernelContext_28;

#line 1473
    (&kernelContext_28)->gen_0 = gen_5;

#line 1473
    (&kernelContext_28)->tables_0 = tables_5;

#line 1473
    (&kernelContext_28)->meshes_0 = meshes_5;

#line 1473
    (&kernelContext_28)->visible_instances_0 = visible_instances_5;

#line 1473
    (&kernelContext_28)->args_0 = args_5;

#line 1473
    (&kernelContext_28)->counts_and_mesh_args_0 = counts_and_mesh_args_5;

#line 1473
    (&kernelContext_28)->visible_count_0 = visible_count_5;

#line 1473
    (&kernelContext_28)->instances_0 = instances_5;

#line 1473
    (&kernelContext_28)->group_state_0 = group_state_5;

#line 1473
    threadgroup array<uint, int(256)> starts_chunk_runs_5;

#line 1473
    (&kernelContext_28)->starts_chunk_runs_0 = &starts_chunk_runs_5;

    uint bucket_12 = thread_4.x;
    if(bucket_12 >= (gen_5->bucket_count_0))
    {
        return;
    }

#line 1478
    uint _S93 = arg_word_0(1U, bucket_12, 1U, &kernelContext_28);

    uint early_1 = atomic_load_explicit((&kernelContext_28)->args_0+_S93, memory_order_relaxed);

#line 1480
    uint _S94 = arg_word_0(2U, bucket_12, 1U, &kernelContext_28);
    uint late_0 = atomic_load_explicit((&kernelContext_28)->args_0+_S94, memory_order_relaxed);

#line 1481
    uint _S95 = mesh_arg_word_0(2U, bucket_12, 1U, &kernelContext_28);
    atomic_store_explicit((&kernelContext_28)->counts_and_mesh_args_0+_S95, late_0, memory_order_relaxed);
    if(late_0 != 0U)
    {

#line 1483
        uint _S96 = count_word_0(2U, bucket_12, &kernelContext_28);

        atomic_store_explicit((&kernelContext_28)->counts_and_mesh_args_0+_S96, 1U, memory_order_relaxed);

#line 1483
    }



    uint drawn_0 = early_1 + late_0;

#line 1487
    uint _S97 = arg_word_0(0U, bucket_12, 1U, &kernelContext_28);
    atomic_store_explicit((&kernelContext_28)->args_0+_S97, drawn_0, memory_order_relaxed);

#line 1488
    uint _S98 = mesh_arg_word_0(0U, bucket_12, 1U, &kernelContext_28);
    atomic_store_explicit((&kernelContext_28)->counts_and_mesh_args_0+_S98, drawn_0, memory_order_relaxed);

#line 1489
    uint _S99 = count_word_0(0U, bucket_12, &kernelContext_28);
    atomic<uint> device* _S100 = (&kernelContext_28)->counts_and_mesh_args_0+_S99;

#line 1490
    int _S101;

#line 1490
    if(drawn_0 != 0U)
    {

#line 1490
        _S101 = int(1);

#line 1490
    }
    else
    {

#line 1490
        _S101 = int(0);

#line 1490
    }

#line 1490
    atomic_store_explicit(_S100, uint(_S101), memory_order_relaxed);
    return;
}

