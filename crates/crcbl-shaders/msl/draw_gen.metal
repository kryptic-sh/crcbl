#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 451 "shaders/draw_gen.slang"
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
};


#line 281
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


#line 1035
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1035
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


#line 1035
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


#line 697
uint bucket_mesh_0(uint bucket_0, KernelContext_0 thread* kernelContext_0)
{
    return kernelContext_0->tables_0[bucket_0];
}


#line 817
uint mesh_arg_word_0(uint bucket_1, uint slot_0, KernelContext_0 thread* kernelContext_1)
{
    return kernelContext_1->gen_0->bucket_count_0 + bucket_1 * 3U + slot_0;
}


#line 730
uint bucket_clusters_0(uint bucket_2, KernelContext_0 thread* kernelContext_2)
{
    return kernelContext_2->tables_0[kernelContext_2->gen_0->bucket_clusters_at_0 + bucket_2];
}


#line 981
uint survivor_count_0(KernelContext_0 thread* kernelContext_3)
{
    return min(kernelContext_3->visible_count_0[0U], kernelContext_3->gen_0->visible_capacity_0);
}


#line 416
struct MeshLevels_0
{
    uint first_group_0;
    uint group_count_0;
    uint first_level_0;
    uint top_level_0;
};


#line 737
MeshLevels_0 mesh_levels_of_0(uint mesh_1, KernelContext_0 thread* kernelContext_4)
{
    uint at_0 = kernelContext_4->gen_0->mesh_levels_at_0 + mesh_1 * 4U;
    thread MeshLevels_0 levels_0;
    (&levels_0)->first_group_0 = kernelContext_4->tables_0[at_0];
    (&levels_0)->group_count_0 = kernelContext_4->tables_0[at_0 + 1U];
    (&levels_0)->first_level_0 = kernelContext_4->tables_0[at_0 + 2U];
    (&levels_0)->top_level_0 = kernelContext_4->tables_0[at_0 + 3U];
    return levels_0;
}


#line 888
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S1 = (((basis_0) * (transpose(basis_0))));

#line 890
    float bound_0 = 0.0f;

#line 890
    uint row_0 = 0U;

    for(;;)
    {

#line 892
        if(row_0 < 3U)
        {
        }
        else
        {

#line 892
            break;
        }
        float _S2 = max(bound_0, abs(_S1[row_0][int(0)]) + abs(_S1[row_0][int(1)]) + abs(_S1[row_0][int(2)]));

#line 892
        uint row_1 = row_0 + 1U;

#line 892
        bound_0 = _S2;

#line 892
        row_0 = row_1;

#line 892
    }



    return sqrt(bound_0);
}


#line 371
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_0;
    float center_y_0;
    float center_z_0;
    float radius_0;
};


#line 755
LevelGroup_0 level_group_at_0(uint group_0, KernelContext_0 thread* kernelContext_5)
{
    uint at_1 = kernelContext_5->gen_0->level_groups_at_0 + group_0 * 6U;
    thread LevelGroup_0 record_0;
    (&record_0)->level_0 = kernelContext_5->tables_0[at_1];
    (&record_0)->error_0 = (as_type<float>((kernelContext_5->tables_0[at_1 + 1U])));
    (&record_0)->center_x_0 = (as_type<float>((kernelContext_5->tables_0[at_1 + 2U])));
    (&record_0)->center_y_0 = (as_type<float>((kernelContext_5->tables_0[at_1 + 3U])));
    (&record_0)->center_z_0 = (as_type<float>((kernelContext_5->tables_0[at_1 + 4U])));
    (&record_0)->radius_0 = (as_type<float>((kernelContext_5->tables_0[at_1 + 5U])));
    return record_0;
}


#line 854
float projected_error_0(float error_1, float3 center_0, float radius_1, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S3 = delta_0.x;

#line 857
    float _S4 = delta_0.y;

#line 857
    float _S5 = delta_0.z;
    float distance_0 = sqrt(_S3 * _S3 + _S4 * _S4 + _S5 * _S5) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 908
uint group_is_expanded_0(float error_2, float3 center_1, float radius_2, float3 eye_1, uint was_0, KernelContext_0 thread* kernelContext_6)
{
    float projected_0 = projected_error_0(error_2, center_1, radius_2, eye_1, kernelContext_6->gen_0->lod_params_0.x);

#line 910
    bool expanded_0;

    if(projected_0 > (kernelContext_6->gen_0->lod_params_0.y))
    {

#line 912
        expanded_0 = true;

#line 912
    }
    else
    {

#line 912
        if(was_0 != 0U)
        {

#line 912
            expanded_0 = projected_0 > (kernelContext_6->gen_0->lod_params_0.z);

#line 912
        }
        else
        {

#line 912
            expanded_0 = false;

#line 912
        }

#line 912
    }

#line 912
    uint _S6;
    if(expanded_0)
    {

#line 913
        _S6 = 1U;

#line 913
    }
    else
    {

#line 913
        _S6 = 0U;

#line 913
    }

#line 913
    return _S6;
}


#line 774
uint level_mesh_at_0(uint level_1, KernelContext_0 thread* kernelContext_7)
{
    return kernelContext_7->tables_0[kernelContext_7->gen_0->level_meshes_at_0 + level_1];
}


#line 708
uint bucket_mode_0(uint bucket_3, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->tables_0[kernelContext_8->gen_0->bucket_modes_at_0 + bucket_3];
}


#line 786
uint route_word_0(uint survivor_0, KernelContext_0 thread* kernelContext_9)
{
    return kernelContext_9->gen_0->visible_capacity_0 + survivor_0;
}


#line 996
uint select_level_0(uint _S7, uint _S8, KernelContext_0 thread* kernelContext_10)
{

#line 996
    GpuInstance_natural_0 device* _S9 = kernelContext_10->instances_0+_S7;

#line 996
    MeshLevels_0 _S10 = mesh_levels_of_0(_S9->mesh_0, kernelContext_10);

#line 954
    float3 _S11 = kernelContext_10->gen_0->camera_position_0.xyz;

#line 954
    matrix<float,int(4),int(4)>  _S12 = matrix<float,int(4),int(4)> (_S9->transform_0.data_0[int(0)][int(0)], _S9->transform_0.data_0[int(1)][int(0)], _S9->transform_0.data_0[int(2)][int(0)], _S9->transform_0.data_0[int(3)][int(0)], _S9->transform_0.data_0[int(0)][int(1)], _S9->transform_0.data_0[int(1)][int(1)], _S9->transform_0.data_0[int(2)][int(1)], _S9->transform_0.data_0[int(3)][int(1)], _S9->transform_0.data_0[int(0)][int(2)], _S9->transform_0.data_0[int(1)][int(2)], _S9->transform_0.data_0[int(2)][int(2)], _S9->transform_0.data_0[int(3)][int(2)], _S9->transform_0.data_0[int(0)][int(3)], _S9->transform_0.data_0[int(1)][int(3)], _S9->transform_0.data_0[int(2)][int(3)], _S9->transform_0.data_0[int(3)][int(3)]);
    float _S13 = max_stretch_0(matrix<float,int(3),int(3)> (_S12[int(0)].xyz, _S12[int(1)].xyz, _S12[int(2)].xyz));
    uint _S14 = _S8 * kernelContext_10->gen_0->group_stride_0;

#line 956
    uint chosen_0 = _S10.top_level_0;

#line 956
    uint i_0 = 0U;

    for(;;)
    {

#line 958
        if(i_0 < (_S10.group_count_0))
        {
        }
        else
        {

#line 958
            break;
        }
        uint at_2 = _S10.first_group_0 + i_0;

#line 960
        LevelGroup_0 _S15 = level_group_at_0(at_2, kernelContext_10);

#line 965
        uint _S16 = _S14 + at_2;

#line 965
        uint _S17 = group_is_expanded_0(_S15.error_0 * _S13, (((float4(_S15.center_x_0, _S15.center_y_0, _S15.center_z_0, 1.0f)) * (_S12))).xyz, _S15.radius_0 * _S13, _S11, *(kernelContext_10->group_state_0+_S16), kernelContext_10);
        *(kernelContext_10->group_state_0+_S16) = _S17;

#line 966
        bool _S18;
        if(_S17 == 1U)
        {

#line 967
            _S18 = (_S15.level_0) < chosen_0;

#line 967
        }
        else
        {

#line 967
            _S18 = false;

#line 967
        }

#line 967
        if(_S18)
        {

#line 967
            chosen_0 = _S15.level_0;

#line 967
        }

#line 958
        i_0 = i_0 + 1U;

#line 958
    }

#line 972
    return chosen_0;
}


#line 972
uint instance_material_mode_0(uint _S19, KernelContext_0 thread* kernelContext_11)
{

#line 722
    return (((kernelContext_11->instances_0+_S19)->flags_1) & 12U) >> 2U;
}


#line 996
[[kernel]] void binMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* tables_1 [[buffer(4)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* args_1 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_1 [[buffer(7)]], uint device* visible_count_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], uint device* group_state_1 [[buffer(8)]])
{

#line 996
    uint routed_0;

#line 996
    thread KernelContext_0 kernelContext_12;

#line 996
    (&kernelContext_12)->gen_0 = gen_1;

#line 996
    (&kernelContext_12)->tables_0 = tables_1;

#line 996
    (&kernelContext_12)->meshes_0 = meshes_1;

#line 996
    (&kernelContext_12)->args_0 = args_1;

#line 996
    (&kernelContext_12)->counts_and_mesh_args_0 = counts_and_mesh_args_1;

#line 996
    (&kernelContext_12)->visible_count_0 = visible_count_1;

#line 996
    (&kernelContext_12)->visible_instances_0 = visible_instances_1;

#line 996
    (&kernelContext_12)->instances_0 = instances_1;

#line 996
    (&kernelContext_12)->group_state_0 = group_state_1;

    uint index_0 = thread_0.x;

#line 1003
    if(index_0 < (gen_1->bucket_count_0))
    {

#line 1003
        uint _S20 = bucket_mesh_0(index_0, &kernelContext_12);

        GpuMesh_0 mesh_2 = (&kernelContext_12)->meshes_0[_S20];
        uint at_3 = index_0 * 5U;
        atomic_store_explicit((&kernelContext_12)->args_0+at_3, mesh_2.index_count_0, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_12)->args_0+(at_3 + 2U), mesh_2.base_index_0, memory_order_relaxed);

#line 1014
        atomic_store_explicit((&kernelContext_12)->args_0+(at_3 + 3U), 0U, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_12)->args_0+(at_3 + 4U), 0U, memory_order_relaxed);

#line 1015
        uint _S21 = mesh_arg_word_0(index_0, 0U, &kernelContext_12);

#line 1022
        atomic<uint> device* _S22 = (&kernelContext_12)->counts_and_mesh_args_0+_S21;

#line 1022
        uint _S23 = bucket_clusters_0(index_0, &kernelContext_12);

#line 1022
        atomic_store_explicit(_S22, _S23, memory_order_relaxed);

#line 1022
        uint _S24 = mesh_arg_word_0(index_0, 2U, &kernelContext_12);
        atomic_store_explicit((&kernelContext_12)->counts_and_mesh_args_0+_S24, 1U, memory_order_relaxed);

#line 1003
    }

#line 1003
    uint _S25 = survivor_count_0(&kernelContext_12);

#line 1028
    if(index_0 >= _S25)
    {
        return;
    }



    uint device* _S26 = (&kernelContext_12)->visible_instances_0+index_0;

#line 1035
    MeshLevels_0 _S27 = mesh_levels_of_0(((&kernelContext_12)->instances_0+*_S26)->mesh_0, &kernelContext_12);

#line 1035
    uint _S28 = select_level_0(*_S26, *_S26, &kernelContext_12);

#line 1035
    uint _S29 = level_mesh_at_0(_S27.first_level_0 + _S28, &kernelContext_12);

#line 1035
    uint _S30 = instance_material_mode_0(*_S26, &kernelContext_12);

#line 1035
    uint bucket_4 = 0U;

#line 1058
    for(;;)
    {

#line 1058
        if(bucket_4 < (gen_1->bucket_count_0))
        {
        }
        else
        {

#line 1058
            routed_0 = 4294967295U;

#line 1058
            break;
        }

#line 1058
        uint _S31 = bucket_mesh_0(bucket_4, &kernelContext_12);

#line 1058
        bool _S32;

        if(_S31 != _S29)
        {

#line 1060
            _S32 = true;

#line 1060
        }
        else
        {

#line 1060
            uint _S33 = bucket_mode_0(bucket_4, &kernelContext_12);

#line 1060
            _S32 = _S33 != _S30;

#line 1060
        }

#line 1060
        if(_S32)
        {
            bucket_4 = bucket_4 + 1U;

#line 1058
            continue;
        }

#line 1058
        uint _S34 = mesh_arg_word_0(bucket_4, 1U, &kernelContext_12);

#line 1071
        uint _S35 = atomic_fetch_add_explicit((&kernelContext_12)->counts_and_mesh_args_0+_S34, 1U, memory_order_relaxed);

#line 1071
        routed_0 = bucket_4;
        break;
    }

#line 1072
    uint _S36 = route_word_0(index_0, &kernelContext_12);

#line 1077
    *((&kernelContext_12)->visible_instances_0+_S36) = routed_0;
    return;
}


#line 793
uint runs_at_0(KernelContext_0 thread* kernelContext_13)
{
    return 2U * kernelContext_13->gen_0->visible_capacity_0;
}


#line 804
uint run_start_word_0(uint bucket_5, KernelContext_0 thread* kernelContext_14)
{
    return 3U * kernelContext_14->gen_0->visible_capacity_0 + bucket_5;
}


uint count_word_0(uint bucket_6)
{
    return bucket_6;
}


#line 1089
[[kernel]] void startsMain(DrawGenParams_0 constant* gen_2 [[buffer(0)]], uint device* tables_2 [[buffer(4)]], GpuMesh_0 device* meshes_2 [[buffer(2)]], atomic<uint> device* args_2 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_2 [[buffer(7)]], uint device* visible_count_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(1)]], uint device* group_state_2 [[buffer(8)]])
{

#line 1089
    thread KernelContext_0 kernelContext_15;

#line 1089
    (&kernelContext_15)->gen_0 = gen_2;

#line 1089
    (&kernelContext_15)->tables_0 = tables_2;

#line 1089
    (&kernelContext_15)->meshes_0 = meshes_2;

#line 1089
    (&kernelContext_15)->args_0 = args_2;

#line 1089
    (&kernelContext_15)->counts_and_mesh_args_0 = counts_and_mesh_args_2;

#line 1089
    (&kernelContext_15)->visible_count_0 = visible_count_2;

#line 1089
    (&kernelContext_15)->visible_instances_0 = visible_instances_2;

#line 1089
    (&kernelContext_15)->instances_0 = instances_2;

#line 1089
    (&kernelContext_15)->group_state_0 = group_state_2;

#line 1089
    uint _S37 = runs_at_0(&kernelContext_15);

#line 1089
    uint bucket_7 = 0U;

#line 1089
    uint start_0 = _S37;


    for(;;)
    {

#line 1092
        if(bucket_7 < ((&kernelContext_15)->gen_0->bucket_count_0))
        {
        }
        else
        {

#line 1092
            break;
        }

#line 1092
        uint _S38 = run_start_word_0(bucket_7, &kernelContext_15);

        *((&kernelContext_15)->visible_instances_0+_S38) = start_0;

#line 1094
        uint _S39 = mesh_arg_word_0(bucket_7, 1U, &kernelContext_15);
        uint routed_1 = atomic_load_explicit((&kernelContext_15)->counts_and_mesh_args_0+_S39, memory_order_relaxed);


        if(routed_1 != 0U)
        {
            atomic_store_explicit((&kernelContext_15)->counts_and_mesh_args_0+count_word_0(bucket_7), 1U, memory_order_relaxed);

#line 1098
        }



        uint start_1 = start_0 + routed_1;

#line 1092
        bucket_7 = bucket_7 + 1U;

#line 1092
        start_0 = start_1;

#line 1092
    }

#line 1104
    return;
}


#line 1115
[[kernel]] void scatterMain(uint3 thread_1 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_3 [[buffer(0)]], uint device* tables_3 [[buffer(4)]], GpuMesh_0 device* meshes_3 [[buffer(2)]], atomic<uint> device* args_3 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_3 [[buffer(7)]], uint device* visible_count_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(1)]], uint device* group_state_3 [[buffer(8)]])
{

#line 1115
    thread KernelContext_0 kernelContext_16;

#line 1115
    (&kernelContext_16)->gen_0 = gen_3;

#line 1115
    (&kernelContext_16)->tables_0 = tables_3;

#line 1115
    (&kernelContext_16)->meshes_0 = meshes_3;

#line 1115
    (&kernelContext_16)->args_0 = args_3;

#line 1115
    (&kernelContext_16)->counts_and_mesh_args_0 = counts_and_mesh_args_3;

#line 1115
    (&kernelContext_16)->visible_count_0 = visible_count_3;

#line 1115
    (&kernelContext_16)->visible_instances_0 = visible_instances_3;

#line 1115
    (&kernelContext_16)->instances_0 = instances_3;

#line 1115
    (&kernelContext_16)->group_state_0 = group_state_3;

    uint index_1 = thread_1.x;

#line 1117
    uint _S40 = survivor_count_0(&kernelContext_16);
    if(index_1 >= _S40)
    {
        return;
    }

#line 1120
    uint _S41 = route_word_0(index_1, &kernelContext_16);

    uint device* _S42 = (&kernelContext_16)->visible_instances_0+_S41;

#line 1122
    uint bucket_8 = *_S42;
    if((*_S42) == 4294967295U)
    {
        return;
    }
    uint slot_1 = atomic_fetch_add_explicit((&kernelContext_16)->args_0+(bucket_8 * 5U + 1U), 1U, memory_order_relaxed);

#line 1127
    uint _S43 = run_start_word_0(bucket_8, &kernelContext_16);
    *((&kernelContext_16)->visible_instances_0+(*((&kernelContext_16)->visible_instances_0+_S43) + slot_1)) = *((&kernelContext_16)->visible_instances_0+index_1);
    return;
}

