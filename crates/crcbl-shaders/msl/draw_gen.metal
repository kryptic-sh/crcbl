#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 266 "shaders/draw_gen.slang"
struct DrawGenParams_0
{
    uint bucket_count_0;
    uint bucket_capacity_0;
    uint visible_capacity_0;
    uint group_stride_0;
    float4 camera_position_0;
    float4 lod_params_0;
};


#line 158
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


#line 536
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 536
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 242
struct MeshLevels_0
{
    uint first_group_0;
    uint group_count_0;
    uint first_level_0;
    uint top_level_0;
};


#line 216
struct LevelGroup_0
{
    uint level_0;
    float error_0;
    float center_x_0;
    float center_y_0;
    float center_z_0;
    float radius_0;
};


#line 507
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
    uint device* group_state_0;
    uint device* level_meshes_0;
    uint device* visible_instances_0;
    uint device* draw_counts_0;
};


#line 461
uint group_is_expanded_0(float error_1, float3 center_0, float radius_1, float3 eye_0, uint was_0, KernelContext_0 thread* kernelContext_0)
{
    float3 delta_0 = eye_0 - center_0;
    float _S1 = delta_0.x;

#line 464
    float _S2 = delta_0.y;

#line 464
    float _S3 = delta_0.z;
    float distance_0 = sqrt(_S1 * _S1 + _S2 * _S2 + _S3 * _S3) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 1U;
    }
    float projected_0 = error_1 * kernelContext_0->gen_0->lod_params_0.x / distance_0;

#line 470
    bool expanded_0;

    if(projected_0 > (kernelContext_0->gen_0->lod_params_0.y))
    {

#line 472
        expanded_0 = true;

#line 472
    }
    else
    {

#line 472
        if(was_0 != 0U)
        {

#line 472
            expanded_0 = projected_0 > (kernelContext_0->gen_0->lod_params_0.z);

#line 472
        }
        else
        {

#line 472
            expanded_0 = false;

#line 472
        }

#line 472
    }

#line 472
    uint _S4;
    if(expanded_0)
    {

#line 473
        _S4 = 1U;

#line 473
    }
    else
    {

#line 473
        _S4 = 0U;

#line 473
    }

#line 473
    return _S4;
}


#line 494
uint select_level_0(const GpuInstance_natural_0 thread* instance_0, uint instance_index_0, KernelContext_0 thread* kernelContext_1)
{
    MeshLevels_0 levels_0 = kernelContext_1->mesh_levels_0[instance_0->mesh_0];
    float3 _S5 = kernelContext_1->gen_0->camera_position_0.xyz;
    uint _S6 = instance_index_0 * kernelContext_1->gen_0->group_stride_0;

#line 498
    uint chosen_0 = levels_0.top_level_0;

#line 498
    uint i_0 = 0U;

    for(;;)
    {

#line 500
        if(i_0 < (levels_0.group_count_0))
        {
        }
        else
        {

#line 500
            break;
        }
        uint at_0 = levels_0.first_group_0 + i_0;
        LevelGroup_0 group_0 = kernelContext_1->level_groups_0[at_0];



        uint _S7 = _S6 + at_0;

#line 507
        uint _S8 = group_is_expanded_0(group_0.error_0, (((float4(group_0.center_x_0, group_0.center_y_0, group_0.center_z_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&instance_0->transform_0)->data_0[int(0)][int(0)], (&instance_0->transform_0)->data_0[int(1)][int(0)], (&instance_0->transform_0)->data_0[int(2)][int(0)], (&instance_0->transform_0)->data_0[int(3)][int(0)], (&instance_0->transform_0)->data_0[int(0)][int(1)], (&instance_0->transform_0)->data_0[int(1)][int(1)], (&instance_0->transform_0)->data_0[int(2)][int(1)], (&instance_0->transform_0)->data_0[int(3)][int(1)], (&instance_0->transform_0)->data_0[int(0)][int(2)], (&instance_0->transform_0)->data_0[int(1)][int(2)], (&instance_0->transform_0)->data_0[int(2)][int(2)], (&instance_0->transform_0)->data_0[int(3)][int(2)], (&instance_0->transform_0)->data_0[int(0)][int(3)], (&instance_0->transform_0)->data_0[int(1)][int(3)], (&instance_0->transform_0)->data_0[int(2)][int(3)], (&instance_0->transform_0)->data_0[int(3)][int(3)])))).xyz, group_0.radius_0, _S5, *(kernelContext_1->group_state_0+_S7), kernelContext_1);
        *(kernelContext_1->group_state_0+_S7) = _S8;

#line 508
        bool _S9;
        if(_S8 == 1U)
        {

#line 509
            _S9 = (group_0.level_0) < chosen_0;

#line 509
        }
        else
        {

#line 509
            _S9 = false;

#line 509
        }

#line 509
        if(_S9)
        {

#line 509
            chosen_0 = group_0.level_0;

#line 509
        }

#line 500
        i_0 = i_0 + 1U;

#line 500
    }

#line 514
    return chosen_0;
}


#line 525
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* bucket_meshes_1 [[buffer(5)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* args_1 [[buffer(7)]], atomic<uint> device* mesh_args_1 [[buffer(10)]], uint device* bucket_clusters_1 [[buffer(9)]], uint device* visible_count_1 [[buffer(4)]], uint device* visible_1 [[buffer(3)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], MeshLevels_0 device* mesh_levels_1 [[buffer(11)]], LevelGroup_0 device* level_groups_1 [[buffer(12)]], uint device* group_state_1 [[buffer(14)]], uint device* level_meshes_1 [[buffer(13)]], uint device* visible_instances_1 [[buffer(6)]], uint device* draw_counts_1 [[buffer(8)]])
{

#line 525
    thread KernelContext_0 kernelContext_2;

#line 525
    (&kernelContext_2)->gen_0 = gen_1;

#line 525
    (&kernelContext_2)->bucket_meshes_0 = bucket_meshes_1;

#line 525
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 525
    (&kernelContext_2)->args_0 = args_1;

#line 525
    (&kernelContext_2)->mesh_args_0 = mesh_args_1;

#line 525
    (&kernelContext_2)->bucket_clusters_0 = bucket_clusters_1;

#line 525
    (&kernelContext_2)->visible_count_0 = visible_count_1;

#line 525
    (&kernelContext_2)->visible_0 = visible_1;

#line 525
    (&kernelContext_2)->instances_0 = instances_1;

#line 525
    (&kernelContext_2)->mesh_levels_0 = mesh_levels_1;

#line 525
    (&kernelContext_2)->level_groups_0 = level_groups_1;

#line 525
    (&kernelContext_2)->group_state_0 = group_state_1;

#line 525
    (&kernelContext_2)->level_meshes_0 = level_meshes_1;

#line 525
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 525
    (&kernelContext_2)->draw_counts_0 = draw_counts_1;

    uint index_0 = thread_0.x;

#line 532
    if(index_0 < (gen_1->bucket_count_0))
    {
        GpuMesh_0 mesh_1 = (&kernelContext_2)->meshes_0[(&kernelContext_2)->bucket_meshes_0[index_0]];
        uint at_1 = index_0 * 5U;
        atomic_store_explicit((&kernelContext_2)->args_0+at_1, mesh_1.index_count_0, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_2)->args_0+(at_1 + 2U), mesh_1.base_index_0, memory_order_relaxed);

#line 543
        atomic_store_explicit((&kernelContext_2)->args_0+(at_1 + 3U), 0U, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_2)->args_0+(at_1 + 4U), 0U, memory_order_relaxed);

#line 550
        uint mesh_at_0 = index_0 * 3U;
        atomic_store_explicit((&kernelContext_2)->mesh_args_0+mesh_at_0, (&kernelContext_2)->bucket_clusters_0[index_0], memory_order_relaxed);
        atomic_store_explicit((&kernelContext_2)->mesh_args_0+(mesh_at_0 + 2U), 1U, memory_order_relaxed);

#line 532
    }

#line 558
    if(index_0 >= (min((&kernelContext_2)->visible_count_0[int(0)], min((&kernelContext_2)->gen_0->visible_capacity_0, (&kernelContext_2)->gen_0->bucket_capacity_0))))
    {
        return;
    }

    uint instance_index_1 = (&kernelContext_2)->visible_0[index_0];
    GpuInstance_natural_0 instance_1 = (&kernelContext_2)->instances_0[instance_index_1];

#line 564
    thread GpuInstance_natural_0 _S10 = instance_1;

#line 572
    MeshLevels_0 _S11 = (&kernelContext_2)->mesh_levels_0[(&_S10)->mesh_0];

#line 572
    _S10 = instance_1;

#line 572
    uint _S12 = select_level_0(&_S10, instance_index_1, &kernelContext_2);

#line 571
    uint _S13 = (&kernelContext_2)->level_meshes_0[_S11.first_level_0 + _S12];

#line 571
    uint bucket_0 = 0U;

#line 576
    for(;;)
    {

#line 576
        if(bucket_0 < (gen_1->bucket_count_0))
        {
        }
        else
        {

#line 576
            break;
        }
        if((&kernelContext_2)->bucket_meshes_0[bucket_0] != _S13)
        {
            bucket_0 = bucket_0 + 1U;

#line 576
            continue;
        }

#line 582
        uint slot_0 = atomic_fetch_add_explicit((&kernelContext_2)->args_0+(bucket_0 * 5U + 1U), 1U, memory_order_relaxed);

#line 589
        uint _S14 = atomic_fetch_add_explicit((&kernelContext_2)->mesh_args_0+(bucket_0 * 3U + 1U), 1U, memory_order_relaxed);

        *((&kernelContext_2)->visible_instances_0+(bucket_0 * (&kernelContext_2)->gen_0->bucket_capacity_0 + slot_0)) = instance_index_1;



        if(slot_0 == 0U)
        {
            *((&kernelContext_2)->draw_counts_0+bucket_0) = 1U;

#line 595
        }



        break;
    }
    return;
}

