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
    uint routes_at_0;
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


#line 1160
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1160
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


#line 1160
struct KernelContext_0
{
    DrawGenParams_0 constant* gen_0;
    uint device* tables_0;
    GpuMesh_0 device* meshes_0;
    atomic<uint> device* args_0;
    atomic<uint> device* counts_and_mesh_args_0;
    uint device* visible_count_0;
    uint device* visible_instances_0;
    GpuInstance_natural_0 device* instances_0;
    uint device* group_state_0;
};


#line 788
uint bucket_mesh_0(uint bucket_0, KernelContext_0 thread* kernelContext_0)
{
    return kernelContext_0->tables_0[bucket_0];
}


#line 904
uint draw_slot_0(uint region_0, uint bucket_1, KernelContext_0 thread* kernelContext_1)
{
    return region_0 * kernelContext_1->gen_0->bucket_count_0 + bucket_1;
}


#line 904
uint draw_slot_1(uint region_1, uint bucket_2, KernelContext_0 thread* kernelContext_2)
{
    return region_1 * kernelContext_2->gen_0->bucket_count_0 + bucket_2;
}


#line 937
uint arg_word_0(uint region_2, uint bucket_3, uint field_0, KernelContext_0 thread* kernelContext_3)
{

#line 937
    uint _S1 = draw_slot_1(region_2, bucket_3, kernelContext_3);

    return _S1 * 5U + field_0;
}


#line 930
uint mesh_arg_word_0(uint region_3, uint bucket_4, uint slot_0, KernelContext_0 thread* kernelContext_4)
{
    return region_3 * 4U * kernelContext_4->gen_0->bucket_count_0 + kernelContext_4->gen_0->bucket_count_0 + bucket_4 * 3U + slot_0;
}


#line 821
uint bucket_clusters_0(uint bucket_5, KernelContext_0 thread* kernelContext_5)
{
    return kernelContext_5->tables_0[kernelContext_5->gen_0->bucket_clusters_at_0 + bucket_5];
}


#line 1101
uint survivor_count_0(KernelContext_0 thread* kernelContext_6)
{
    return min(kernelContext_6->visible_count_0[0U], kernelContext_6->gen_0->visible_capacity_0);
}


#line 490
struct MeshLevels_0
{
    uint first_group_0;
    uint group_count_0;
    uint first_level_0;
    uint top_level_0;
};


#line 828
MeshLevels_0 mesh_levels_of_0(uint mesh_1, KernelContext_0 thread* kernelContext_7)
{
    uint at_0 = kernelContext_7->gen_0->mesh_levels_at_0 + mesh_1 * 4U;
    thread MeshLevels_0 levels_0;
    (&levels_0)->first_group_0 = kernelContext_7->tables_0[at_0];
    (&levels_0)->group_count_0 = kernelContext_7->tables_0[at_0 + 1U];
    (&levels_0)->first_level_0 = kernelContext_7->tables_0[at_0 + 2U];
    (&levels_0)->top_level_0 = kernelContext_7->tables_0[at_0 + 3U];
    return levels_0;
}


#line 1008
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S2 = (((basis_0) * (transpose(basis_0))));

#line 1010
    float bound_0 = 0.0f;

#line 1010
    uint row_0 = 0U;

    for(;;)
    {

#line 1012
        if(row_0 < 3U)
        {
        }
        else
        {

#line 1012
            break;
        }
        float _S3 = max(bound_0, abs(_S2[row_0][int(0)]) + abs(_S2[row_0][int(1)]) + abs(_S2[row_0][int(2)]));

#line 1012
        uint row_1 = row_0 + 1U;

#line 1012
        bound_0 = _S3;

#line 1012
        row_0 = row_1;

#line 1012
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


#line 846
LevelGroup_0 level_group_at_0(uint group_0, KernelContext_0 thread* kernelContext_8)
{
    uint at_1 = kernelContext_8->gen_0->level_groups_at_0 + group_0 * 6U;
    thread LevelGroup_0 record_0;
    (&record_0)->level_0 = kernelContext_8->tables_0[at_1];
    (&record_0)->error_0 = (as_type<float>((kernelContext_8->tables_0[at_1 + 1U])));
    (&record_0)->center_x_0 = (as_type<float>((kernelContext_8->tables_0[at_1 + 2U])));
    (&record_0)->center_y_0 = (as_type<float>((kernelContext_8->tables_0[at_1 + 3U])));
    (&record_0)->center_z_0 = (as_type<float>((kernelContext_8->tables_0[at_1 + 4U])));
    (&record_0)->radius_0 = (as_type<float>((kernelContext_8->tables_0[at_1 + 5U])));
    return record_0;
}


#line 974
float projected_error_0(float error_1, float3 center_0, float radius_1, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S4 = delta_0.x;

#line 977
    float _S5 = delta_0.y;

#line 977
    float _S6 = delta_0.z;
    float distance_0 = sqrt(_S4 * _S4 + _S5 * _S5 + _S6 * _S6) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 1028
uint group_is_expanded_0(float error_2, float3 center_1, float radius_2, float3 eye_1, uint was_0, KernelContext_0 thread* kernelContext_9)
{
    float projected_0 = projected_error_0(error_2, center_1, radius_2, eye_1, kernelContext_9->gen_0->lod_params_0.x);

#line 1030
    bool expanded_0;

    if(projected_0 > (kernelContext_9->gen_0->lod_params_0.y))
    {

#line 1032
        expanded_0 = true;

#line 1032
    }
    else
    {

#line 1032
        if(was_0 != 0U)
        {

#line 1032
            expanded_0 = projected_0 > (kernelContext_9->gen_0->lod_params_0.z);

#line 1032
        }
        else
        {

#line 1032
            expanded_0 = false;

#line 1032
        }

#line 1032
    }

#line 1032
    uint _S7;
    if(expanded_0)
    {

#line 1033
        _S7 = 1U;

#line 1033
    }
    else
    {

#line 1033
        _S7 = 0U;

#line 1033
    }

#line 1033
    return _S7;
}


#line 865
uint level_mesh_at_0(uint level_1, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->tables_0[kernelContext_10->gen_0->level_meshes_at_0 + level_1];
}


#line 881
uint route_bucket_0(uint mesh_2, uint mode_1, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->tables_0[kernelContext_11->gen_0->routes_at_0 + mesh_2 * 4U + mode_1];
}



uint route_word_0(uint survivor_0, KernelContext_0 thread* kernelContext_12)
{
    return kernelContext_12->gen_0->visible_capacity_0 + survivor_0;
}


#line 1116
uint select_level_0(uint _S8, uint _S9, KernelContext_0 thread* kernelContext_13)
{

#line 1116
    GpuInstance_natural_0 device* _S10 = kernelContext_13->instances_0+_S8;

#line 1116
    MeshLevels_0 _S11 = mesh_levels_of_0(_S10->mesh_0, kernelContext_13);

#line 1074
    float3 _S12 = kernelContext_13->gen_0->camera_position_0.xyz;

#line 1074
    matrix<float,int(4),int(4)>  _S13 = matrix<float,int(4),int(4)> (_S10->transform_0.data_0[int(0)][int(0)], _S10->transform_0.data_0[int(1)][int(0)], _S10->transform_0.data_0[int(2)][int(0)], _S10->transform_0.data_0[int(3)][int(0)], _S10->transform_0.data_0[int(0)][int(1)], _S10->transform_0.data_0[int(1)][int(1)], _S10->transform_0.data_0[int(2)][int(1)], _S10->transform_0.data_0[int(3)][int(1)], _S10->transform_0.data_0[int(0)][int(2)], _S10->transform_0.data_0[int(1)][int(2)], _S10->transform_0.data_0[int(2)][int(2)], _S10->transform_0.data_0[int(3)][int(2)], _S10->transform_0.data_0[int(0)][int(3)], _S10->transform_0.data_0[int(1)][int(3)], _S10->transform_0.data_0[int(2)][int(3)], _S10->transform_0.data_0[int(3)][int(3)]);
    float _S14 = max_stretch_0(matrix<float,int(3),int(3)> (_S13[int(0)].xyz, _S13[int(1)].xyz, _S13[int(2)].xyz));
    uint _S15 = _S9 * kernelContext_13->gen_0->group_stride_0;

#line 1076
    uint chosen_0 = _S11.top_level_0;

#line 1076
    uint i_0 = 0U;

    for(;;)
    {

#line 1078
        if(i_0 < (_S11.group_count_0))
        {
        }
        else
        {

#line 1078
            break;
        }
        uint at_2 = _S11.first_group_0 + i_0;

#line 1080
        LevelGroup_0 _S16 = level_group_at_0(at_2, kernelContext_13);

#line 1085
        uint _S17 = _S15 + at_2;

#line 1085
        uint _S18 = group_is_expanded_0(_S16.error_0 * _S14, (((float4(_S16.center_x_0, _S16.center_y_0, _S16.center_z_0, 1.0f)) * (_S13))).xyz, _S16.radius_0 * _S14, _S12, *(kernelContext_13->group_state_0+_S17), kernelContext_13);
        *(kernelContext_13->group_state_0+_S17) = _S18;

#line 1086
        bool _S19;
        if(_S18 == 1U)
        {

#line 1087
            _S19 = (_S16.level_0) < chosen_0;

#line 1087
        }
        else
        {

#line 1087
            _S19 = false;

#line 1087
        }

#line 1087
        if(_S19)
        {

#line 1087
            chosen_0 = _S16.level_0;

#line 1087
        }

#line 1078
        i_0 = i_0 + 1U;

#line 1078
    }

#line 1092
    return chosen_0;
}


#line 1092
uint instance_material_mode_0(uint _S20, KernelContext_0 thread* kernelContext_14)
{

#line 813
    return (((kernelContext_14->instances_0+_S20)->flags_1) & 12U) >> 2U;
}


#line 1116
[[kernel]] void binMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* tables_1 [[buffer(4)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* args_1 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_1 [[buffer(7)]], uint device* visible_count_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], uint device* group_state_1 [[buffer(8)]])
{

#line 1116
    thread KernelContext_0 kernelContext_15;

#line 1116
    (&kernelContext_15)->gen_0 = gen_1;

#line 1116
    (&kernelContext_15)->tables_0 = tables_1;

#line 1116
    (&kernelContext_15)->meshes_0 = meshes_1;

#line 1116
    (&kernelContext_15)->args_0 = args_1;

#line 1116
    (&kernelContext_15)->counts_and_mesh_args_0 = counts_and_mesh_args_1;

#line 1116
    (&kernelContext_15)->visible_count_0 = visible_count_1;

#line 1116
    (&kernelContext_15)->visible_instances_0 = visible_instances_1;

#line 1116
    (&kernelContext_15)->instances_0 = instances_1;

#line 1116
    (&kernelContext_15)->group_state_0 = group_state_1;

    uint index_0 = thread_0.x;

#line 1118
    uint face_0;

#line 1123
    if(index_0 < (gen_1->bucket_count_0))
    {

#line 1123
        uint _S21 = bucket_mesh_0(index_0, &kernelContext_15);

        GpuMesh_0 _S22 = (&kernelContext_15)->meshes_0[_S21];

#line 1125
        face_0 = 0U;


        for(;;)
        {

#line 1128
            if(face_0 < ((&kernelContext_15)->gen_0->draw_regions_0))
            {
            }
            else
            {

#line 1128
                break;
            }

#line 1128
            uint _S23 = arg_word_0(face_0, index_0, 0U, &kernelContext_15);

            atomic_store_explicit((&kernelContext_15)->args_0+_S23, _S22.index_count_0, memory_order_relaxed);

#line 1130
            uint _S24 = arg_word_0(face_0, index_0, 2U, &kernelContext_15);
            atomic_store_explicit((&kernelContext_15)->args_0+_S24, _S22.base_index_0, memory_order_relaxed);

#line 1131
            uint _S25 = arg_word_0(face_0, index_0, 3U, &kernelContext_15);

#line 1138
            atomic_store_explicit((&kernelContext_15)->args_0+_S25, 0U, memory_order_relaxed);

#line 1138
            uint _S26 = arg_word_0(face_0, index_0, 4U, &kernelContext_15);
            atomic_store_explicit((&kernelContext_15)->args_0+_S26, 0U, memory_order_relaxed);

#line 1139
            uint _S27 = mesh_arg_word_0(face_0, index_0, 0U, &kernelContext_15);

#line 1146
            atomic<uint> device* _S28 = (&kernelContext_15)->counts_and_mesh_args_0+_S27;

#line 1146
            uint _S29 = bucket_clusters_0(index_0, &kernelContext_15);

#line 1146
            atomic_store_explicit(_S28, _S29, memory_order_relaxed);

#line 1146
            uint _S30 = mesh_arg_word_0(face_0, index_0, 2U, &kernelContext_15);
            atomic_store_explicit((&kernelContext_15)->counts_and_mesh_args_0+_S30, 1U, memory_order_relaxed);

#line 1128
            face_0 = face_0 + 1U;

#line 1128
        }

#line 1123
    }

#line 1123
    uint _S31 = survivor_count_0(&kernelContext_15);

#line 1153
    if(index_0 >= _S31)
    {
        return;
    }



    uint device* _S32 = (&kernelContext_15)->visible_instances_0+index_0;

#line 1160
    uint entry_0 = *_S32;


    uint instance_index_0 = (*_S32) & 16777215U;

#line 1163
    MeshLevels_0 _S33 = mesh_levels_of_0(((&kernelContext_15)->instances_0+instance_index_0)->mesh_0, &kernelContext_15);

#line 1163
    uint _S34 = select_level_0(instance_index_0, instance_index_0, &kernelContext_15);

#line 1163
    uint _S35 = level_mesh_at_0(_S33.first_level_0 + _S34, &kernelContext_15);

#line 1163
    uint _S36 = instance_material_mode_0(instance_index_0, &kernelContext_15);

#line 1163
    uint _S37 = route_bucket_0(_S35, _S36, &kernelContext_15);

#line 1182
    if(_S37 != 4294967295U)
    {

#line 1190
        if(((&kernelContext_15)->gen_0->mode_0) == 2U)
        {

#line 1190
            face_0 = 0U;


            for(;;)
            {

#line 1193
                if(face_0 < 6U)
                {
                }
                else
                {

#line 1193
                    break;
                }
                if(((entry_0 >> (24U + face_0)) & 1U) != 0U)
                {

#line 1195
                    uint _S38 = mesh_arg_word_0(1U + face_0, _S37, 1U, &kernelContext_15);


                    uint _S39 = atomic_fetch_add_explicit((&kernelContext_15)->counts_and_mesh_args_0+_S38, 1U, memory_order_relaxed);

#line 1195
                }

#line 1193
                face_0 = face_0 + 1U;

#line 1193
            }

#line 1190
        }
        else
        {

#line 1190
            uint _S40 = mesh_arg_word_0(0U, _S37, 1U, &kernelContext_15);

#line 1207
            uint _S41 = atomic_fetch_add_explicit((&kernelContext_15)->counts_and_mesh_args_0+_S40, 1U, memory_order_relaxed);

#line 1207
            bool _S42;
            if(((&kernelContext_15)->gen_0->mode_0) == 1U)
            {

#line 1208
                _S42 = (entry_0 & 2147483648U) == 0U;

#line 1208
            }
            else
            {

#line 1208
                _S42 = false;

#line 1208
            }

#line 1208
            if(_S42)
            {

#line 1208
                uint _S43 = mesh_arg_word_0(1U, _S37, 1U, &kernelContext_15);


                uint _S44 = atomic_fetch_add_explicit((&kernelContext_15)->counts_and_mesh_args_0+_S43, 1U, memory_order_relaxed);

#line 1208
            }

#line 1190
        }

#line 1182
    }

#line 1182
    uint _S45 = route_word_0(index_0, &kernelContext_15);

#line 1218
    *((&kernelContext_15)->visible_instances_0+_S45) = _S37;
    return;
}


#line 915
uint run_start_word_0(uint region_4, uint bucket_6, KernelContext_0 thread* kernelContext_16)
{
    uint _S46 = 3U * kernelContext_16->gen_0->visible_capacity_0;

#line 917
    uint _S47 = draw_slot_0(region_4, bucket_6, kernelContext_16);

#line 917
    return _S46 + _S47;
}




uint count_word_0(uint region_5, uint bucket_7, KernelContext_0 thread* kernelContext_17)
{
    return region_5 * 4U * kernelContext_17->gen_0->bucket_count_0 + bucket_7;
}


#line 895
uint runs_at_0(KernelContext_0 thread* kernelContext_18)
{
    return 2U * kernelContext_18->gen_0->visible_capacity_0;
}


#line 1230
[[kernel]] void startsMain(DrawGenParams_0 constant* gen_2 [[buffer(0)]], uint device* tables_2 [[buffer(4)]], GpuMesh_0 device* meshes_2 [[buffer(2)]], atomic<uint> device* args_2 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_2 [[buffer(7)]], uint device* visible_count_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(1)]], uint device* group_state_2 [[buffer(8)]])
{

#line 1230
    thread KernelContext_0 kernelContext_19;

#line 1230
    (&kernelContext_19)->gen_0 = gen_2;

#line 1230
    (&kernelContext_19)->tables_0 = tables_2;

#line 1230
    (&kernelContext_19)->meshes_0 = meshes_2;

#line 1230
    (&kernelContext_19)->args_0 = args_2;

#line 1230
    (&kernelContext_19)->counts_and_mesh_args_0 = counts_and_mesh_args_2;

#line 1230
    (&kernelContext_19)->visible_count_0 = visible_count_2;

#line 1230
    (&kernelContext_19)->visible_instances_0 = visible_instances_2;

#line 1230
    (&kernelContext_19)->instances_0 = instances_2;

#line 1230
    (&kernelContext_19)->group_state_0 = group_state_2;

#line 1230
    uint face_1;

#line 1230
    uint face_start_0;

    if((gen_2->mode_0) == 2U)
    {



        uint _S48 = (&kernelContext_19)->gen_0->face_runs_at_0;

#line 1237
        face_1 = 0U;

#line 1237
        face_start_0 = _S48;
        for(;;)
        {

#line 1238
            if(face_1 < 6U)
            {
            }
            else
            {

#line 1238
                break;
            }
            uint _S49 = 1U + face_1;

#line 1240
            uint bucket_8 = 0U;
            for(;;)
            {

#line 1241
                if(bucket_8 < ((&kernelContext_19)->gen_0->bucket_count_0))
                {
                }
                else
                {

#line 1241
                    break;
                }

#line 1241
                uint _S50 = run_start_word_0(_S49, bucket_8, &kernelContext_19);

                *((&kernelContext_19)->visible_instances_0+_S50) = face_start_0;

#line 1243
                uint _S51 = mesh_arg_word_0(_S49, bucket_8, 1U, &kernelContext_19);

                uint reached_0 = atomic_load_explicit((&kernelContext_19)->counts_and_mesh_args_0+_S51, memory_order_relaxed);
                if(reached_0 != 0U)
                {

#line 1246
                    uint _S52 = count_word_0(_S49, bucket_8, &kernelContext_19);

                    atomic_store_explicit((&kernelContext_19)->counts_and_mesh_args_0+_S52, 1U, memory_order_relaxed);

#line 1246
                }



                uint face_start_1 = face_start_0 + reached_0;

#line 1241
                bucket_8 = bucket_8 + 1U;

#line 1241
                face_start_0 = face_start_1;

#line 1241
            }

#line 1238
            face_1 = face_1 + 1U;

#line 1238
        }

#line 1253
        return;
    }

#line 1253
    uint _S53 = runs_at_0(&kernelContext_19);

#line 1253
    face_1 = 0U;

#line 1253
    face_start_0 = _S53;



    for(;;)
    {

#line 1257
        if(face_1 < ((&kernelContext_19)->gen_0->bucket_count_0))
        {
        }
        else
        {

#line 1257
            break;
        }

#line 1257
        uint _S54 = run_start_word_0(0U, face_1, &kernelContext_19);

        *((&kernelContext_19)->visible_instances_0+_S54) = face_start_0;

#line 1259
        uint _S55 = mesh_arg_word_0(0U, face_1, 1U, &kernelContext_19);
        uint routed_0 = atomic_load_explicit((&kernelContext_19)->counts_and_mesh_args_0+_S55, memory_order_relaxed);


        if(routed_0 != 0U)
        {

#line 1263
            uint _S56 = count_word_0(0U, face_1, &kernelContext_19);

            atomic_store_explicit((&kernelContext_19)->counts_and_mesh_args_0+_S56, 1U, memory_order_relaxed);

#line 1263
        }



        if((gen_2->mode_0) == 1U)
        {

#line 1267
            uint _S57 = mesh_arg_word_0(1U, face_1, 1U, &kernelContext_19);

#line 1273
            uint early_0 = atomic_load_explicit((&kernelContext_19)->counts_and_mesh_args_0+_S57, memory_order_relaxed);

#line 1273
            uint _S58 = run_start_word_0(1U, face_1, &kernelContext_19);
            *((&kernelContext_19)->visible_instances_0+_S58) = face_start_0;

#line 1274
            uint _S59 = run_start_word_0(2U, face_1, &kernelContext_19);
            *((&kernelContext_19)->visible_instances_0+_S59) = face_start_0 + early_0;
            if(early_0 != 0U)
            {

#line 1276
                uint _S60 = count_word_0(1U, face_1, &kernelContext_19);

                atomic_store_explicit((&kernelContext_19)->counts_and_mesh_args_0+_S60, 1U, memory_order_relaxed);

#line 1276
            }

#line 1267
        }

#line 1281
        uint start_0 = face_start_0 + routed_0;

#line 1257
        face_1 = face_1 + 1U;

#line 1257
        face_start_0 = start_0;

#line 1257
    }

#line 1283
    return;
}


#line 1294
[[kernel]] void scatterMain(uint3 thread_1 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_3 [[buffer(0)]], uint device* tables_3 [[buffer(4)]], GpuMesh_0 device* meshes_3 [[buffer(2)]], atomic<uint> device* args_3 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_3 [[buffer(7)]], uint device* visible_count_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(1)]], uint device* group_state_3 [[buffer(8)]])
{

#line 1294
    thread KernelContext_0 kernelContext_20;

#line 1294
    (&kernelContext_20)->gen_0 = gen_3;

#line 1294
    (&kernelContext_20)->tables_0 = tables_3;

#line 1294
    (&kernelContext_20)->meshes_0 = meshes_3;

#line 1294
    (&kernelContext_20)->args_0 = args_3;

#line 1294
    (&kernelContext_20)->counts_and_mesh_args_0 = counts_and_mesh_args_3;

#line 1294
    (&kernelContext_20)->visible_count_0 = visible_count_3;

#line 1294
    (&kernelContext_20)->visible_instances_0 = visible_instances_3;

#line 1294
    (&kernelContext_20)->instances_0 = instances_3;

#line 1294
    (&kernelContext_20)->group_state_0 = group_state_3;

    uint index_1 = thread_1.x;

#line 1296
    uint _S61 = survivor_count_0(&kernelContext_20);
    if(index_1 >= _S61)
    {
        return;
    }

#line 1299
    uint _S62 = route_word_0(index_1, &kernelContext_20);

    uint device* _S63 = (&kernelContext_20)->visible_instances_0+_S62;

#line 1301
    uint bucket_9 = *_S63;
    if((*_S63) == 4294967295U)
    {
        return;
    }
    uint device* _S64 = (&kernelContext_20)->visible_instances_0+index_1;

#line 1306
    uint entry_1 = *_S64;
    uint instance_index_1 = (*_S64) & 16777215U;

#line 1307
    uint face_2;
    if(((&kernelContext_20)->gen_0->mode_0) == 2U)
    {

#line 1308
        face_2 = 0U;

        for(;;)
        {

#line 1310
            if(face_2 < 6U)
            {
            }
            else
            {

#line 1310
                break;
            }
            if(((entry_1 >> (24U + face_2)) & 1U) == 0U)
            {
                face_2 = face_2 + 1U;

#line 1310
                continue;
            }

#line 1316
            uint region_6 = 1U + face_2;

#line 1316
            uint _S65 = arg_word_0(region_6, bucket_9, 1U, &kernelContext_20);
            uint face_slot_0 = atomic_fetch_add_explicit((&kernelContext_20)->args_0+_S65, 1U, memory_order_relaxed);

#line 1317
            uint _S66 = run_start_word_0(region_6, bucket_9, &kernelContext_20);
            *((&kernelContext_20)->visible_instances_0+(*((&kernelContext_20)->visible_instances_0+_S66) + face_slot_0)) = instance_index_1;

#line 1310
            face_2 = face_2 + 1U;

#line 1310
        }

#line 1321
        return;
    }

#line 1321
    bool _S67;


    if(((&kernelContext_20)->gen_0->mode_0) == 1U)
    {

#line 1324
        _S67 = (entry_1 & 2147483648U) != 0U;

#line 1324
    }
    else
    {

#line 1324
        _S67 = false;

#line 1324
    }

#line 1324
    if(_S67)
    {
        return;
    }
    if(((&kernelContext_20)->gen_0->mode_0) == 1U)
    {

#line 1328
        face_2 = 1U;

#line 1328
    }
    else
    {

#line 1328
        face_2 = 0U;

#line 1328
    }

#line 1328
    uint _S68 = arg_word_0(face_2, bucket_9, 1U, &kernelContext_20);
    uint slot_1 = atomic_fetch_add_explicit((&kernelContext_20)->args_0+_S68, 1U, memory_order_relaxed);

#line 1329
    uint _S69 = run_start_word_0(face_2, bucket_9, &kernelContext_20);
    *((&kernelContext_20)->visible_instances_0+(*((&kernelContext_20)->visible_instances_0+_S69) + slot_1)) = instance_index_1;
    return;
}


#line 1342
[[kernel]] void lateScatterMain(uint3 thread_2 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_4 [[buffer(0)]], uint device* tables_4 [[buffer(4)]], GpuMesh_0 device* meshes_4 [[buffer(2)]], atomic<uint> device* args_4 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_4 [[buffer(7)]], uint device* visible_count_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(1)]], uint device* group_state_4 [[buffer(8)]])
{

#line 1342
    thread KernelContext_0 kernelContext_21;

#line 1342
    (&kernelContext_21)->gen_0 = gen_4;

#line 1342
    (&kernelContext_21)->tables_0 = tables_4;

#line 1342
    (&kernelContext_21)->meshes_0 = meshes_4;

#line 1342
    (&kernelContext_21)->args_0 = args_4;

#line 1342
    (&kernelContext_21)->counts_and_mesh_args_0 = counts_and_mesh_args_4;

#line 1342
    (&kernelContext_21)->visible_count_0 = visible_count_4;

#line 1342
    (&kernelContext_21)->visible_instances_0 = visible_instances_4;

#line 1342
    (&kernelContext_21)->instances_0 = instances_4;

#line 1342
    (&kernelContext_21)->group_state_0 = group_state_4;

    uint index_2 = thread_2.x;

#line 1344
    uint _S70 = survivor_count_0(&kernelContext_21);
    if(index_2 >= _S70)
    {
        return;
    }
    uint device* _S71 = (&kernelContext_21)->visible_instances_0+index_2;

#line 1349
    uint entry_2 = *_S71;
    if(((*_S71) & 1073741824U) == 0U)
    {
        return;
    }

#line 1352
    uint _S72 = route_word_0(index_2, &kernelContext_21);

    uint device* _S73 = (&kernelContext_21)->visible_instances_0+_S72;

#line 1354
    uint bucket_10 = *_S73;
    if((*_S73) == 4294967295U)
    {
        return;
    }

#line 1357
    uint _S74 = arg_word_0(2U, bucket_10, 1U, &kernelContext_21);

    uint slot_2 = atomic_fetch_add_explicit((&kernelContext_21)->args_0+_S74, 1U, memory_order_relaxed);

#line 1359
    uint _S75 = run_start_word_0(2U, bucket_10, &kernelContext_21);
    *((&kernelContext_21)->visible_instances_0+(*((&kernelContext_21)->visible_instances_0+_S75) + slot_2)) = entry_2 & 16777215U;

    return;
}


#line 1373
[[kernel]] void lateFinishMain(uint3 thread_3 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_5 [[buffer(0)]], uint device* tables_5 [[buffer(4)]], GpuMesh_0 device* meshes_5 [[buffer(2)]], atomic<uint> device* args_5 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_5 [[buffer(7)]], uint device* visible_count_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(1)]], uint device* group_state_5 [[buffer(8)]])
{

#line 1373
    thread KernelContext_0 kernelContext_22;

#line 1373
    (&kernelContext_22)->gen_0 = gen_5;

#line 1373
    (&kernelContext_22)->tables_0 = tables_5;

#line 1373
    (&kernelContext_22)->meshes_0 = meshes_5;

#line 1373
    (&kernelContext_22)->args_0 = args_5;

#line 1373
    (&kernelContext_22)->counts_and_mesh_args_0 = counts_and_mesh_args_5;

#line 1373
    (&kernelContext_22)->visible_count_0 = visible_count_5;

#line 1373
    (&kernelContext_22)->visible_instances_0 = visible_instances_5;

#line 1373
    (&kernelContext_22)->instances_0 = instances_5;

#line 1373
    (&kernelContext_22)->group_state_0 = group_state_5;

    uint bucket_11 = thread_3.x;
    if(bucket_11 >= (gen_5->bucket_count_0))
    {
        return;
    }

#line 1378
    uint _S76 = arg_word_0(1U, bucket_11, 1U, &kernelContext_22);

    uint early_1 = atomic_load_explicit((&kernelContext_22)->args_0+_S76, memory_order_relaxed);

#line 1380
    uint _S77 = arg_word_0(2U, bucket_11, 1U, &kernelContext_22);
    uint late_0 = atomic_load_explicit((&kernelContext_22)->args_0+_S77, memory_order_relaxed);

#line 1381
    uint _S78 = mesh_arg_word_0(2U, bucket_11, 1U, &kernelContext_22);
    atomic_store_explicit((&kernelContext_22)->counts_and_mesh_args_0+_S78, late_0, memory_order_relaxed);
    if(late_0 != 0U)
    {

#line 1383
        uint _S79 = count_word_0(2U, bucket_11, &kernelContext_22);

        atomic_store_explicit((&kernelContext_22)->counts_and_mesh_args_0+_S79, 1U, memory_order_relaxed);

#line 1383
    }



    uint drawn_0 = early_1 + late_0;

#line 1387
    uint _S80 = arg_word_0(0U, bucket_11, 1U, &kernelContext_22);
    atomic_store_explicit((&kernelContext_22)->args_0+_S80, drawn_0, memory_order_relaxed);

#line 1388
    uint _S81 = mesh_arg_word_0(0U, bucket_11, 1U, &kernelContext_22);
    atomic_store_explicit((&kernelContext_22)->counts_and_mesh_args_0+_S81, drawn_0, memory_order_relaxed);

#line 1389
    uint _S82 = count_word_0(0U, bucket_11, &kernelContext_22);
    atomic<uint> device* _S83 = (&kernelContext_22)->counts_and_mesh_args_0+_S82;

#line 1390
    int _S84;

#line 1390
    if(drawn_0 != 0U)
    {

#line 1390
        _S84 = int(1);

#line 1390
    }
    else
    {

#line 1390
        _S84 = int(0);

#line 1390
    }

#line 1390
    atomic_store_explicit(_S83, uint(_S84), memory_order_relaxed);
    return;
}

