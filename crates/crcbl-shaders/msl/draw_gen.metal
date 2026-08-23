#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 349 "shaders/draw_gen.slang"
struct DrawGenParams_0
{
    uint bucket_count_0;
    uint bucket_capacity_0;
    uint visible_capacity_0;
    uint group_stride_0;
    uint bucket_clusters_at_0;
    uint mesh_levels_at_0;
    uint level_groups_at_0;
    uint level_meshes_at_0;
    float4 camera_position_0;
    float4 lod_params_0;
};


#line 202
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
};


#line 847
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 847
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 847
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


#line 563
uint bucket_mesh_0(uint bucket_0, KernelContext_0 thread* kernelContext_0)
{
    return kernelContext_0->tables_0[bucket_0];
}


#line 640
uint mesh_arg_word_0(uint bucket_1, uint slot_0, KernelContext_0 thread* kernelContext_1)
{
    return kernelContext_1->gen_0->bucket_count_0 + bucket_1 * 3U + slot_0;
}


#line 573
uint bucket_clusters_0(uint bucket_2, KernelContext_0 thread* kernelContext_2)
{
    return kernelContext_2->tables_0[kernelContext_2->gen_0->bucket_clusters_at_0 + bucket_2];
}


#line 314
struct MeshLevels_0
{
    uint first_group_0;
    uint group_count_0;
    uint first_level_0;
    uint top_level_0;
};


#line 580
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


#line 711
float max_stretch_0(matrix<float,int(3),int(3)>  basis_0)
{
    matrix<float,int(3),int(3)>  _S1 = (((basis_0) * (transpose(basis_0))));

#line 713
    float bound_0 = 0.0f;

#line 713
    uint row_0 = 0U;

    for(;;)
    {

#line 715
        if(row_0 < 3U)
        {
        }
        else
        {

#line 715
            break;
        }
        float _S2 = max(bound_0, abs(_S1[row_0][int(0)]) + abs(_S1[row_0][int(1)]) + abs(_S1[row_0][int(2)]));

#line 715
        uint row_1 = row_0 + 1U;

#line 715
        bound_0 = _S2;

#line 715
        row_0 = row_1;

#line 715
    }



    return sqrt(bound_0);
}


#line 269
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_0;
    float center_y_0;
    float center_z_0;
    float radius_0;
};


#line 598
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


#line 677
float projected_error_0(float error_1, float3 center_0, float radius_1, float3 eye_0, float pixels_per_unit_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S3 = delta_0.x;

#line 680
    float _S4 = delta_0.y;

#line 680
    float _S5 = delta_0.z;
    float distance_0 = sqrt(_S3 * _S3 + _S4 * _S4 + _S5 * _S5) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}


#line 731
uint group_is_expanded_0(float error_2, float3 center_1, float radius_2, float3 eye_1, uint was_0, KernelContext_0 thread* kernelContext_5)
{
    float projected_0 = projected_error_0(error_2, center_1, radius_2, eye_1, kernelContext_5->gen_0->lod_params_0.x);

#line 733
    bool expanded_0;

    if(projected_0 > (kernelContext_5->gen_0->lod_params_0.y))
    {

#line 735
        expanded_0 = true;

#line 735
    }
    else
    {

#line 735
        if(was_0 != 0U)
        {

#line 735
            expanded_0 = projected_0 > (kernelContext_5->gen_0->lod_params_0.z);

#line 735
        }
        else
        {

#line 735
            expanded_0 = false;

#line 735
        }

#line 735
    }

#line 735
    uint _S6;
    if(expanded_0)
    {

#line 736
        _S6 = 1U;

#line 736
    }
    else
    {

#line 736
        _S6 = 0U;

#line 736
    }

#line 736
    return _S6;
}


#line 774
uint select_level_0(const GpuInstance_natural_0 thread* instance_0, uint instance_index_0, KernelContext_0 thread* kernelContext_6)
{

#line 774
    MeshLevels_0 _S7 = mesh_levels_of_0(instance_0->mesh_0, kernelContext_6);


    float3 _S8 = kernelContext_6->gen_0->camera_position_0.xyz;

#line 777
    matrix<float,int(4),int(4)>  _S9 = matrix<float,int(4),int(4)> ((&instance_0->transform_0)->data_0[int(0)][int(0)], (&instance_0->transform_0)->data_0[int(1)][int(0)], (&instance_0->transform_0)->data_0[int(2)][int(0)], (&instance_0->transform_0)->data_0[int(3)][int(0)], (&instance_0->transform_0)->data_0[int(0)][int(1)], (&instance_0->transform_0)->data_0[int(1)][int(1)], (&instance_0->transform_0)->data_0[int(2)][int(1)], (&instance_0->transform_0)->data_0[int(3)][int(1)], (&instance_0->transform_0)->data_0[int(0)][int(2)], (&instance_0->transform_0)->data_0[int(1)][int(2)], (&instance_0->transform_0)->data_0[int(2)][int(2)], (&instance_0->transform_0)->data_0[int(3)][int(2)], (&instance_0->transform_0)->data_0[int(0)][int(3)], (&instance_0->transform_0)->data_0[int(1)][int(3)], (&instance_0->transform_0)->data_0[int(2)][int(3)], (&instance_0->transform_0)->data_0[int(3)][int(3)]);
    float _S10 = max_stretch_0(matrix<float,int(3),int(3)> (float3((&instance_0->transform_0)->data_0[int(0)][int(0)], (&instance_0->transform_0)->data_0[int(1)][int(0)], (&instance_0->transform_0)->data_0[int(2)][int(0)]), float3((&instance_0->transform_0)->data_0[int(0)][int(1)], (&instance_0->transform_0)->data_0[int(1)][int(1)], (&instance_0->transform_0)->data_0[int(2)][int(1)]), float3((&instance_0->transform_0)->data_0[int(0)][int(2)], (&instance_0->transform_0)->data_0[int(1)][int(2)], (&instance_0->transform_0)->data_0[int(2)][int(2)])));
    uint _S11 = instance_index_0 * kernelContext_6->gen_0->group_stride_0;

#line 779
    uint chosen_0 = _S7.top_level_0;

#line 779
    uint i_0 = 0U;

    for(;;)
    {

#line 781
        if(i_0 < (_S7.group_count_0))
        {
        }
        else
        {

#line 781
            break;
        }
        uint at_2 = _S7.first_group_0 + i_0;

#line 783
        LevelGroup_0 _S12 = level_group_at_0(at_2, kernelContext_6);

#line 788
        uint _S13 = _S11 + at_2;

#line 788
        uint _S14 = group_is_expanded_0(_S12.error_0 * _S10, (((float4(_S12.center_x_0, _S12.center_y_0, _S12.center_z_0, 1.0f)) * (_S9))).xyz, _S12.radius_0 * _S10, _S8, *(kernelContext_6->group_state_0+_S13), kernelContext_6);
        *(kernelContext_6->group_state_0+_S13) = _S14;

#line 789
        bool _S15;
        if(_S14 == 1U)
        {

#line 790
            _S15 = (_S12.level_0) < chosen_0;

#line 790
        }
        else
        {

#line 790
            _S15 = false;

#line 790
        }

#line 790
        if(_S15)
        {

#line 790
            chosen_0 = _S12.level_0;

#line 790
        }

#line 781
        i_0 = i_0 + 1U;

#line 781
    }

#line 795
    return chosen_0;
}


#line 617
uint level_mesh_at_0(uint level_1, KernelContext_0 thread* kernelContext_7)
{
    return kernelContext_7->tables_0[kernelContext_7->gen_0->level_meshes_at_0 + level_1];
}


#line 627
uint bucket_base_0(uint bucket_3, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->gen_0->visible_capacity_0 + bucket_3 * kernelContext_8->gen_0->bucket_capacity_0;
}


uint count_word_0(uint bucket_4)
{
    return bucket_4;
}


#line 806
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* tables_1 [[buffer(4)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* args_1 [[buffer(6)]], atomic<uint> device* counts_and_mesh_args_1 [[buffer(7)]], uint device* visible_count_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], uint device* group_state_1 [[buffer(8)]])
{

#line 806
    thread KernelContext_0 kernelContext_9;

#line 806
    (&kernelContext_9)->gen_0 = gen_1;

#line 806
    (&kernelContext_9)->tables_0 = tables_1;

#line 806
    (&kernelContext_9)->meshes_0 = meshes_1;

#line 806
    (&kernelContext_9)->args_0 = args_1;

#line 806
    (&kernelContext_9)->counts_and_mesh_args_0 = counts_and_mesh_args_1;

#line 806
    (&kernelContext_9)->visible_count_0 = visible_count_1;

#line 806
    (&kernelContext_9)->visible_instances_0 = visible_instances_1;

#line 806
    (&kernelContext_9)->instances_0 = instances_1;

#line 806
    (&kernelContext_9)->group_state_0 = group_state_1;

    uint index_0 = thread_0.x;

#line 813
    if(index_0 < (gen_1->bucket_count_0))
    {

#line 813
        uint _S16 = bucket_mesh_0(index_0, &kernelContext_9);

        GpuMesh_0 mesh_2 = (&kernelContext_9)->meshes_0[_S16];
        uint at_3 = index_0 * 5U;
        atomic_store_explicit((&kernelContext_9)->args_0+at_3, mesh_2.index_count_0, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_9)->args_0+(at_3 + 2U), mesh_2.base_index_0, memory_order_relaxed);

#line 824
        atomic_store_explicit((&kernelContext_9)->args_0+(at_3 + 3U), 0U, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_9)->args_0+(at_3 + 4U), 0U, memory_order_relaxed);

#line 825
        uint _S17 = mesh_arg_word_0(index_0, 0U, &kernelContext_9);

#line 832
        atomic<uint> device* _S18 = (&kernelContext_9)->counts_and_mesh_args_0+_S17;

#line 832
        uint _S19 = bucket_clusters_0(index_0, &kernelContext_9);

#line 832
        atomic_store_explicit(_S18, _S19, memory_order_relaxed);

#line 832
        uint _S20 = mesh_arg_word_0(index_0, 2U, &kernelContext_9);
        atomic_store_explicit((&kernelContext_9)->counts_and_mesh_args_0+_S20, 1U, memory_order_relaxed);

#line 813
    }

#line 840
    if(index_0 >= (min((&kernelContext_9)->visible_count_0[0U], min((&kernelContext_9)->gen_0->visible_capacity_0, (&kernelContext_9)->gen_0->bucket_capacity_0))))
    {
        return;
    }



    uint device* _S21 = (&kernelContext_9)->visible_instances_0+index_0;

#line 847
    uint instance_index_1 = *_S21;
    GpuInstance_natural_0 instance_1 = (&kernelContext_9)->instances_0[*_S21];

#line 848
    thread GpuInstance_natural_0 _S22 = instance_1;

#line 848
    MeshLevels_0 _S23 = mesh_levels_of_0((&_S22)->mesh_0, &kernelContext_9);

#line 848
    _S22 = instance_1;

#line 848
    uint _S24 = select_level_0(&_S22, *_S21, &kernelContext_9);

#line 848
    uint _S25 = level_mesh_at_0(_S23.first_level_0 + _S24, &kernelContext_9);

#line 848
    uint bucket_5 = 0U;

#line 860
    for(;;)
    {

#line 860
        if(bucket_5 < (gen_1->bucket_count_0))
        {
        }
        else
        {

#line 860
            break;
        }

#line 860
        uint _S26 = bucket_mesh_0(bucket_5, &kernelContext_9);

        if(_S26 != _S25)
        {
            bucket_5 = bucket_5 + 1U;

#line 860
            continue;
        }

#line 866
        uint slot_1 = atomic_fetch_add_explicit((&kernelContext_9)->args_0+(bucket_5 * 5U + 1U), 1U, memory_order_relaxed);

#line 866
        uint _S27 = mesh_arg_word_0(bucket_5, 1U, &kernelContext_9);

#line 873
        uint _S28 = atomic_fetch_add_explicit((&kernelContext_9)->counts_and_mesh_args_0+_S27, 1U, memory_order_relaxed);

#line 873
        uint _S29 = bucket_base_0(bucket_5, &kernelContext_9);

        *((&kernelContext_9)->visible_instances_0+(_S29 + slot_1)) = instance_index_1;



        if(slot_1 == 0U)
        {
            atomic_store_explicit((&kernelContext_9)->counts_and_mesh_args_0+count_word_0(bucket_5), 1U, memory_order_relaxed);

#line 879
        }



        break;
    }
    return;
}

