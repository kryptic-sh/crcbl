#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 402 "shaders/draw_gen.slang"
struct DrawGenParams_0
{
    uint bucket_count_0;
    uint bucket_capacity_0;
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


#line 232
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


#line 934
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 934
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


#line 934
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


#line 627
uint bucket_mesh_0(uint bucket_0, KernelContext_0 thread* kernelContext_0)
{
    return kernelContext_0->tables_0[bucket_0];
}


#line 727
uint mesh_arg_word_0(uint bucket_1, uint slot_0, KernelContext_0 thread* kernelContext_1)
{
    return kernelContext_1->gen_0->bucket_count_0 + bucket_1 * 3U + slot_0;
}


#line 660
uint bucket_clusters_0(uint bucket_2, KernelContext_0 thread* kernelContext_2)
{
    return kernelContext_2->tables_0[kernelContext_2->gen_0->bucket_clusters_at_0 + bucket_2];
}


#line 367
struct MeshLevels_0
{
    uint first_group_0;
    uint group_count_0;
    uint first_level_0;
    uint top_level_0;
};


#line 667
MeshLevels_0 mesh_levels_of_0(uint mesh_1, KernelContext_0 thread* kernelContext_3)
{
    uint at_0 = kernelContext_3->gen_0->mesh_levels_at_0 + mesh_1 * 4U;
    thread MeshLevels_0 levels_0;
    (&levels_0)->first_group_0 = kernelContext_3->tables_0[at_0];
    (&levels_0)->group_count_0 = kernelContext_3->tables_0[at_0 + 1U];
    (&levels_0)->first_level_0 = kernelContext_3->tables_0[at_0 + 2U];
    (&levels_0)->top_level_0 = kernelContext_3->tables_0[at_0 + 3U];
    return levels_0;
}


#line 798
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S1 = (((basis_0) * (transpose(basis_0))));

#line 800
    float bound_0 = 0.0f;

#line 800
    uint row_0 = 0U;

    for(;;)
    {

#line 802
        if(row_0 < 3U)
        {
        }
        else
        {

#line 802
            break;
        }
        float _S2 = max(bound_0, abs(_S1[row_0][int(0)]) + abs(_S1[row_0][int(1)]) + abs(_S1[row_0][int(2)]));

#line 802
        uint row_1 = row_0 + 1U;

#line 802
        bound_0 = _S2;

#line 802
        row_0 = row_1;

#line 802
    }



    return sqrt(bound_0);
}


#line 322
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_0;
    float center_y_0;
    float center_z_0;
    float radius_0;
};


#line 685
LevelGroup_0 level_group_at_0(uint group_0, KernelContext_0 thread* kernelContext_4)
{
    uint at_1 = kernelContext_4->gen_0->level_groups_at_0 + group_0 * 6U;
    thread LevelGroup_0 record_0;
    (&record_0)->level_0 = kernelContext_4->tables_0[at_1];
    (&record_0)->error_0 = (as_type<float>((kernelContext_4->tables_0[at_1 + 1U])));
    (&record_0)->center_x_0 = (as_type<float>((kernelContext_4->tables_0[at_1 + 2U])));
    (&record_0)->center_y_0 = (as_type<float>((kernelContext_4->tables_0[at_1 + 3U])));
    (&record_0)->center_z_0 = (as_type<float>((kernelContext_4->tables_0[at_1 + 4U])));
    (&record_0)->radius_0 = (as_type<float>((kernelContext_4->tables_0[at_1 + 5U])));
    return record_0;
}


#line 764
float projected_error_0(float error_1, float3 center_0, float radius_1, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S3 = delta_0.x;

#line 767
    float _S4 = delta_0.y;

#line 767
    float _S5 = delta_0.z;
    float distance_0 = sqrt(_S3 * _S3 + _S4 * _S4 + _S5 * _S5) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 818
uint group_is_expanded_0(float error_2, float3 center_1, float radius_2, float3 eye_1, uint was_0, KernelContext_0 thread* kernelContext_5)
{
    float projected_0 = projected_error_0(error_2, center_1, radius_2, eye_1, kernelContext_5->gen_0->lod_params_0.x);

#line 820
    bool expanded_0;

    if(projected_0 > (kernelContext_5->gen_0->lod_params_0.y))
    {

#line 822
        expanded_0 = true;

#line 822
    }
    else
    {

#line 822
        if(was_0 != 0U)
        {

#line 822
            expanded_0 = projected_0 > (kernelContext_5->gen_0->lod_params_0.z);

#line 822
        }
        else
        {

#line 822
            expanded_0 = false;

#line 822
        }

#line 822
    }

#line 822
    uint _S6;
    if(expanded_0)
    {

#line 823
        _S6 = 1U;

#line 823
    }
    else
    {

#line 823
        _S6 = 0U;

#line 823
    }

#line 823
    return _S6;
}


#line 704
uint level_mesh_at_0(uint level_1, KernelContext_0 thread* kernelContext_6)
{
    return kernelContext_6->tables_0[kernelContext_6->gen_0->level_meshes_at_0 + level_1];
}


#line 638
uint bucket_mode_0(uint bucket_3, KernelContext_0 thread* kernelContext_7)
{
    return kernelContext_7->tables_0[kernelContext_7->gen_0->bucket_modes_at_0 + bucket_3];
}


#line 714
uint bucket_base_0(uint bucket_4, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->gen_0->visible_capacity_0 + bucket_4 * kernelContext_8->gen_0->bucket_capacity_0;
}


uint count_word_0(uint bucket_5)
{
    return bucket_5;
}


#line 893
uint select_level_0(uint _S7, uint _S8, KernelContext_0 thread* kernelContext_9)
{

#line 893
    GpuInstance_natural_0 device* _S9 = kernelContext_9->instances_0+_S7;

#line 893
    MeshLevels_0 _S10 = mesh_levels_of_0(_S9->mesh_0, kernelContext_9);

#line 864
    float3 _S11 = kernelContext_9->gen_0->camera_position_0.xyz;

#line 864
    matrix<float,int(4),int(4)>  _S12 = matrix<float,int(4),int(4)> (_S9->transform_0.data_0[int(0)][int(0)], _S9->transform_0.data_0[int(1)][int(0)], _S9->transform_0.data_0[int(2)][int(0)], _S9->transform_0.data_0[int(3)][int(0)], _S9->transform_0.data_0[int(0)][int(1)], _S9->transform_0.data_0[int(1)][int(1)], _S9->transform_0.data_0[int(2)][int(1)], _S9->transform_0.data_0[int(3)][int(1)], _S9->transform_0.data_0[int(0)][int(2)], _S9->transform_0.data_0[int(1)][int(2)], _S9->transform_0.data_0[int(2)][int(2)], _S9->transform_0.data_0[int(3)][int(2)], _S9->transform_0.data_0[int(0)][int(3)], _S9->transform_0.data_0[int(1)][int(3)], _S9->transform_0.data_0[int(2)][int(3)], _S9->transform_0.data_0[int(3)][int(3)]);
    float _S13 = max_stretch_0(matrix<float,int(3),int(3)> (_S12[int(0)].xyz, _S12[int(1)].xyz, _S12[int(2)].xyz));
    uint _S14 = _S8 * kernelContext_9->gen_0->group_stride_0;

#line 866
    uint chosen_0 = _S10.top_level_0;

#line 866
    uint i_0 = 0U;

    for(;;)
    {

#line 868
        if(i_0 < (_S10.group_count_0))
        {
        }
        else
        {

#line 868
            break;
        }
        uint at_2 = _S10.first_group_0 + i_0;

#line 870
        LevelGroup_0 _S15 = level_group_at_0(at_2, kernelContext_9);

#line 875
        uint _S16 = _S14 + at_2;

#line 875
        uint _S17 = group_is_expanded_0(_S15.error_0 * _S13, (((float4(_S15.center_x_0, _S15.center_y_0, _S15.center_z_0, 1.0f)) * (_S12))).xyz, _S15.radius_0 * _S13, _S11, *(kernelContext_9->group_state_0+_S16), kernelContext_9);
        *(kernelContext_9->group_state_0+_S16) = _S17;

#line 876
        bool _S18;
        if(_S17 == 1U)
        {

#line 877
            _S18 = (_S15.level_0) < chosen_0;

#line 877
        }
        else
        {

#line 877
            _S18 = false;

#line 877
        }

#line 877
        if(_S18)
        {

#line 877
            chosen_0 = _S15.level_0;

#line 877
        }

#line 868
        i_0 = i_0 + 1U;

#line 868
    }

#line 882
    return chosen_0;
}


#line 882
uint instance_material_mode_0(uint _S19, KernelContext_0 thread* kernelContext_10)
{

#line 652
    return (((kernelContext_10->instances_0+_S19)->flags_1) & 12U) >> 2U;
}


#line 893
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* tables_1 [[buffer(4)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* args_1 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_1 [[buffer(7)]], uint device* visible_count_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], uint device* group_state_1 [[buffer(8)]])
{

#line 893
    thread KernelContext_0 kernelContext_11;

#line 893
    (&kernelContext_11)->gen_0 = gen_1;

#line 893
    (&kernelContext_11)->tables_0 = tables_1;

#line 893
    (&kernelContext_11)->meshes_0 = meshes_1;

#line 893
    (&kernelContext_11)->args_0 = args_1;

#line 893
    (&kernelContext_11)->counts_and_mesh_args_0 = counts_and_mesh_args_1;

#line 893
    (&kernelContext_11)->visible_count_0 = visible_count_1;

#line 893
    (&kernelContext_11)->visible_instances_0 = visible_instances_1;

#line 893
    (&kernelContext_11)->instances_0 = instances_1;

#line 893
    (&kernelContext_11)->group_state_0 = group_state_1;

    uint index_0 = thread_0.x;

#line 900
    if(index_0 < (gen_1->bucket_count_0))
    {

#line 900
        uint _S20 = bucket_mesh_0(index_0, &kernelContext_11);

        GpuMesh_0 mesh_2 = (&kernelContext_11)->meshes_0[_S20];
        uint at_3 = index_0 * 5U;
        atomic_store_explicit((&kernelContext_11)->args_0+at_3, mesh_2.index_count_0, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_11)->args_0+(at_3 + 2U), mesh_2.base_index_0, memory_order_relaxed);

#line 911
        atomic_store_explicit((&kernelContext_11)->args_0+(at_3 + 3U), 0U, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_11)->args_0+(at_3 + 4U), 0U, memory_order_relaxed);

#line 912
        uint _S21 = mesh_arg_word_0(index_0, 0U, &kernelContext_11);

#line 919
        atomic<uint> device* _S22 = (&kernelContext_11)->counts_and_mesh_args_0+_S21;

#line 919
        uint _S23 = bucket_clusters_0(index_0, &kernelContext_11);

#line 919
        atomic_store_explicit(_S22, _S23, memory_order_relaxed);

#line 919
        uint _S24 = mesh_arg_word_0(index_0, 2U, &kernelContext_11);
        atomic_store_explicit((&kernelContext_11)->counts_and_mesh_args_0+_S24, 1U, memory_order_relaxed);

#line 900
    }

#line 927
    if(index_0 >= (min((&kernelContext_11)->visible_count_0[0U], min((&kernelContext_11)->gen_0->visible_capacity_0, (&kernelContext_11)->gen_0->bucket_capacity_0))))
    {
        return;
    }



    uint device* _S25 = (&kernelContext_11)->visible_instances_0+index_0;

#line 934
    uint instance_index_0 = *_S25;

#line 934
    MeshLevels_0 _S26 = mesh_levels_of_0(((&kernelContext_11)->instances_0+*_S25)->mesh_0, &kernelContext_11);

#line 934
    uint _S27 = select_level_0(*_S25, *_S25, &kernelContext_11);

#line 934
    uint _S28 = level_mesh_at_0(_S26.first_level_0 + _S27, &kernelContext_11);

#line 934
    uint _S29 = instance_material_mode_0(*_S25, &kernelContext_11);

#line 934
    uint bucket_6 = 0U;

#line 953
    for(;;)
    {

#line 953
        if(bucket_6 < (gen_1->bucket_count_0))
        {
        }
        else
        {

#line 953
            break;
        }

#line 953
        uint _S30 = bucket_mesh_0(bucket_6, &kernelContext_11);

#line 953
        bool _S31;

        if(_S30 != _S28)
        {

#line 955
            _S31 = true;

#line 955
        }
        else
        {

#line 955
            uint _S32 = bucket_mode_0(bucket_6, &kernelContext_11);

#line 955
            _S31 = _S32 != _S29;

#line 955
        }

#line 955
        if(_S31)
        {
            bucket_6 = bucket_6 + 1U;

#line 953
            continue;
        }

#line 959
        uint slot_1 = atomic_fetch_add_explicit((&kernelContext_11)->args_0+(bucket_6 * 5U + 1U), 1U, memory_order_relaxed);

#line 959
        uint _S33 = mesh_arg_word_0(bucket_6, 1U, &kernelContext_11);

#line 966
        uint _S34 = atomic_fetch_add_explicit((&kernelContext_11)->counts_and_mesh_args_0+_S33, 1U, memory_order_relaxed);

#line 966
        uint _S35 = bucket_base_0(bucket_6, &kernelContext_11);

        *((&kernelContext_11)->visible_instances_0+(_S35 + slot_1)) = instance_index_0;



        if(slot_1 == 0U)
        {
            atomic_store_explicit((&kernelContext_11)->counts_and_mesh_args_0+count_word_0(bucket_6), 1U, memory_order_relaxed);

#line 972
        }



        break;
    }
    return;
}

