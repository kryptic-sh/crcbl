#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 250 "shaders/draw_gen.slang"
struct DrawGenParams_0
{
    uint bucket_count_0;
    uint bucket_capacity_0;
    uint visible_capacity_0;
    uint pad0_0;
    float4 camera_position_0;
    float4 lod_params_0;
};


#line 142
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


#line 469
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 469
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 226
struct MeshLevels_0
{
    uint first_group_0;
    uint group_count_0;
    uint first_level_0;
    uint top_level_0;
};


#line 200
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_0;
    float center_y_0;
    float center_z_0;
    float radius_0;
};


#line 529
struct KernelContext_0
{
    DrawGenParams_0 constant* gen_0;
    uint device* bucket_meshes_0;
    GpuMesh_0 device* meshes_0;
    atomic<uint> device* args_0;
    atomic<uint> device* mesh_args_0;
    uint device* bucket_clusters_0;
    uint device* visible_count_0;
    uint device* visible_0;
    GpuInstance_natural_0 device* instances_0;
    MeshLevels_0 device* mesh_levels_0;
    LevelGroup_0 device* level_groups_0;
    uint device* level_meshes_0;
    uint device* visible_instances_0;
    uint device* draw_counts_0;
};


#line 401
uint group_is_expanded_0(float error_1, float3 center_0, float radius_1, float3 eye_0, KernelContext_0 thread* kernelContext_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S1 = delta_0.x;

#line 404
    float _S2 = delta_0.y;

#line 404
    float _S3 = delta_0.z;
    float distance_0 = sqrt(_S1 * _S1 + _S2 * _S2 + _S3 * _S3) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 1U;
    }

#line 408
    uint _S4;

    if((error_1 * kernelContext_0->gen_0->lod_params_0.x / distance_0) > (kernelContext_0->gen_0->lod_params_0.y))
    {

#line 410
        _S4 = 1U;

#line 410
    }
    else
    {

#line 410
        _S4 = 0U;

#line 410
    }

#line 410
    return _S4;
}


#line 425
uint uniform_level_0(const GpuInstance_natural_0 thread* instance_0, KernelContext_0 thread* kernelContext_1)
{
    MeshLevels_0 levels_0 = kernelContext_1->mesh_levels_0[instance_0->mesh_0];
    float3 _S5 = kernelContext_1->gen_0->camera_position_0.xyz;

#line 428
    uint chosen_0 = levels_0.top_level_0;

#line 428
    uint i_0 = 0U;

    for(;;)
    {

#line 430
        if(i_0 < (levels_0.group_count_0))
        {
        }
        else
        {

#line 430
            break;
        }
        LevelGroup_0 group_0 = kernelContext_1->level_groups_0[levels_0.first_group_0 + i_0];



        if((group_0.level_0) >= chosen_0)
        {
            i_0 = i_0 + 1U;

#line 430
            continue;
        }

#line 430
        uint _S6 = group_is_expanded_0(group_0.error_0, (((float4(group_0.center_x_0, group_0.center_y_0, group_0.center_z_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&instance_0->transform_0)->data_0[int(0)][int(0)], (&instance_0->transform_0)->data_0[int(1)][int(0)], (&instance_0->transform_0)->data_0[int(2)][int(0)], (&instance_0->transform_0)->data_0[int(3)][int(0)], (&instance_0->transform_0)->data_0[int(0)][int(1)], (&instance_0->transform_0)->data_0[int(1)][int(1)], (&instance_0->transform_0)->data_0[int(2)][int(1)], (&instance_0->transform_0)->data_0[int(3)][int(1)], (&instance_0->transform_0)->data_0[int(0)][int(2)], (&instance_0->transform_0)->data_0[int(1)][int(2)], (&instance_0->transform_0)->data_0[int(2)][int(2)], (&instance_0->transform_0)->data_0[int(3)][int(2)], (&instance_0->transform_0)->data_0[int(0)][int(3)], (&instance_0->transform_0)->data_0[int(1)][int(3)], (&instance_0->transform_0)->data_0[int(2)][int(3)], (&instance_0->transform_0)->data_0[int(3)][int(3)])))).xyz, group_0.radius_0, _S5, kernelContext_1);

#line 430
        uint chosen_1;

#line 442
        if(_S6 == 1U)
        {

#line 442
            chosen_1 = group_0.level_0;

#line 442
        }
        else
        {

#line 442
            chosen_1 = chosen_0;

#line 442
        }

#line 442
        chosen_0 = chosen_1;

#line 430
        i_0 = i_0 + 1U;

#line 430
    }

#line 447
    return chosen_0;
}


#line 458
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* bucket_meshes_1 [[buffer(5)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* args_1 [[buffer(7)]], atomic<uint> device* mesh_args_1 [[buffer(10)]], uint device* bucket_clusters_1 [[buffer(9)]], uint device* visible_count_1 [[buffer(4)]], uint device* visible_1 [[buffer(3)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], MeshLevels_0 device* mesh_levels_1 [[buffer(11)]], LevelGroup_0 device* level_groups_1 [[buffer(12)]], uint device* level_meshes_1 [[buffer(13)]], uint device* visible_instances_1 [[buffer(6)]], uint device* draw_counts_1 [[buffer(8)]])
{

#line 458
    thread KernelContext_0 kernelContext_2;

#line 458
    (&kernelContext_2)->gen_0 = gen_1;

#line 458
    (&kernelContext_2)->bucket_meshes_0 = bucket_meshes_1;

#line 458
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 458
    (&kernelContext_2)->args_0 = args_1;

#line 458
    (&kernelContext_2)->mesh_args_0 = mesh_args_1;

#line 458
    (&kernelContext_2)->bucket_clusters_0 = bucket_clusters_1;

#line 458
    (&kernelContext_2)->visible_count_0 = visible_count_1;

#line 458
    (&kernelContext_2)->visible_0 = visible_1;

#line 458
    (&kernelContext_2)->instances_0 = instances_1;

#line 458
    (&kernelContext_2)->mesh_levels_0 = mesh_levels_1;

#line 458
    (&kernelContext_2)->level_groups_0 = level_groups_1;

#line 458
    (&kernelContext_2)->level_meshes_0 = level_meshes_1;

#line 458
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 458
    (&kernelContext_2)->draw_counts_0 = draw_counts_1;

    uint index_0 = thread_0.x;

#line 465
    if(index_0 < (gen_1->bucket_count_0))
    {
        GpuMesh_0 mesh_1 = (&kernelContext_2)->meshes_0[(&kernelContext_2)->bucket_meshes_0[index_0]];
        uint at_0 = index_0 * 5U;
        atomic_store_explicit((&kernelContext_2)->args_0+at_0, mesh_1.index_count_0, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_2)->args_0+(at_0 + 2U), mesh_1.base_index_0, memory_order_relaxed);

#line 476
        atomic_store_explicit((&kernelContext_2)->args_0+(at_0 + 3U), 0U, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_2)->args_0+(at_0 + 4U), 0U, memory_order_relaxed);

#line 483
        uint mesh_at_0 = index_0 * 3U;
        atomic_store_explicit((&kernelContext_2)->mesh_args_0+mesh_at_0, (&kernelContext_2)->bucket_clusters_0[index_0], memory_order_relaxed);
        atomic_store_explicit((&kernelContext_2)->mesh_args_0+(mesh_at_0 + 2U), 1U, memory_order_relaxed);

#line 465
    }

#line 491
    if(index_0 >= (min((&kernelContext_2)->visible_count_0[int(0)], min((&kernelContext_2)->gen_0->visible_capacity_0, (&kernelContext_2)->gen_0->bucket_capacity_0))))
    {
        return;
    }

    uint instance_index_0 = (&kernelContext_2)->visible_0[index_0];
    GpuInstance_natural_0 instance_1 = (&kernelContext_2)->instances_0[instance_index_0];

#line 497
    thread GpuInstance_natural_0 _S7 = instance_1;

#line 504
    MeshLevels_0 _S8 = (&kernelContext_2)->mesh_levels_0[(&_S7)->mesh_0];

#line 504
    _S7 = instance_1;

#line 504
    uint _S9 = uniform_level_0(&_S7, &kernelContext_2);

#line 504
    uint _S10 = (&kernelContext_2)->level_meshes_0[_S8.first_level_0 + _S9];

#line 504
    uint bucket_0 = 0U;



    for(;;)
    {

#line 508
        if(bucket_0 < (gen_1->bucket_count_0))
        {
        }
        else
        {

#line 508
            break;
        }
        if((&kernelContext_2)->bucket_meshes_0[bucket_0] != _S10)
        {
            bucket_0 = bucket_0 + 1U;

#line 508
            continue;
        }

#line 514
        uint slot_0 = atomic_fetch_add_explicit((&kernelContext_2)->args_0+(bucket_0 * 5U + 1U), 1U, memory_order_relaxed);

#line 521
        uint _S11 = atomic_fetch_add_explicit((&kernelContext_2)->mesh_args_0+(bucket_0 * 3U + 1U), 1U, memory_order_relaxed);

#line 529
        *((&kernelContext_2)->visible_instances_0+(bucket_0 * (&kernelContext_2)->gen_0->bucket_capacity_0 + slot_0)) = instance_index_0;



        if(slot_0 == 0U)
        {
            *((&kernelContext_2)->draw_counts_0+bucket_0) = 1U;

#line 533
        }



        break;
    }
    return;
}

