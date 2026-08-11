#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 180 "shaders/draw_gen.slang"
struct DrawGenParams_0
{
    uint bucket_count_0;
    uint bucket_capacity_0;
    uint visible_capacity_0;
    uint pad0_0;
};


#line 122
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


#line 300
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 300
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 353
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
    uint device* visible_instances_0;
    uint device* draw_counts_0;
};


#line 289
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], DrawGenParams_0 constant* gen_1 [[buffer(0)]], uint device* bucket_meshes_1 [[buffer(5)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* args_1 [[buffer(7)]], atomic<uint> device* mesh_args_1 [[buffer(10)]], uint device* bucket_clusters_1 [[buffer(9)]], uint device* visible_count_1 [[buffer(4)]], uint device* visible_1 [[buffer(3)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], uint device* visible_instances_1 [[buffer(6)]], uint device* draw_counts_1 [[buffer(8)]])
{

#line 289
    thread KernelContext_0 kernelContext_0;

#line 289
    (&kernelContext_0)->gen_0 = gen_1;

#line 289
    (&kernelContext_0)->bucket_meshes_0 = bucket_meshes_1;

#line 289
    (&kernelContext_0)->meshes_0 = meshes_1;

#line 289
    (&kernelContext_0)->args_0 = args_1;

#line 289
    (&kernelContext_0)->mesh_args_0 = mesh_args_1;

#line 289
    (&kernelContext_0)->bucket_clusters_0 = bucket_clusters_1;

#line 289
    (&kernelContext_0)->visible_count_0 = visible_count_1;

#line 289
    (&kernelContext_0)->visible_0 = visible_1;

#line 289
    (&kernelContext_0)->instances_0 = instances_1;

#line 289
    (&kernelContext_0)->visible_instances_0 = visible_instances_1;

#line 289
    (&kernelContext_0)->draw_counts_0 = draw_counts_1;

    uint index_0 = thread_0.x;

#line 296
    if(index_0 < (gen_1->bucket_count_0))
    {
        GpuMesh_0 mesh_1 = (&kernelContext_0)->meshes_0[(&kernelContext_0)->bucket_meshes_0[index_0]];
        uint at_0 = index_0 * 5U;
        atomic_store_explicit((&kernelContext_0)->args_0+at_0, mesh_1.index_count_0, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_0)->args_0+(at_0 + 2U), mesh_1.base_index_0, memory_order_relaxed);

#line 307
        atomic_store_explicit((&kernelContext_0)->args_0+(at_0 + 3U), 0U, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_0)->args_0+(at_0 + 4U), 0U, memory_order_relaxed);

#line 314
        uint mesh_at_0 = index_0 * 3U;
        atomic_store_explicit((&kernelContext_0)->mesh_args_0+mesh_at_0, (&kernelContext_0)->bucket_clusters_0[index_0], memory_order_relaxed);
        atomic_store_explicit((&kernelContext_0)->mesh_args_0+(mesh_at_0 + 2U), 1U, memory_order_relaxed);

#line 296
    }

#line 322
    if(index_0 >= (min((&kernelContext_0)->visible_count_0[int(0)], min((&kernelContext_0)->gen_0->visible_capacity_0, (&kernelContext_0)->gen_0->bucket_capacity_0))))
    {
        return;
    }

    uint instance_index_0 = (&kernelContext_0)->visible_0[index_0];
    GpuInstance_natural_0 _S1 = (&kernelContext_0)->instances_0[instance_index_0];

#line 328
    uint bucket_0 = 0U;



    for(;;)
    {

#line 332
        if(bucket_0 < (gen_1->bucket_count_0))
        {
        }
        else
        {

#line 332
            break;
        }
        if((&kernelContext_0)->bucket_meshes_0[bucket_0] != (_S1.mesh_0))
        {
            bucket_0 = bucket_0 + 1U;

#line 332
            continue;
        }

#line 338
        uint slot_0 = atomic_fetch_add_explicit((&kernelContext_0)->args_0+(bucket_0 * 5U + 1U), 1U, memory_order_relaxed);

#line 345
        uint _S2 = atomic_fetch_add_explicit((&kernelContext_0)->mesh_args_0+(bucket_0 * 3U + 1U), 1U, memory_order_relaxed);

#line 353
        *((&kernelContext_0)->visible_instances_0+(bucket_0 * (&kernelContext_0)->gen_0->bucket_capacity_0 + slot_0)) = instance_index_0;



        if(slot_0 == 0U)
        {
            *((&kernelContext_0)->draw_counts_0+bucket_0) = 1U;

#line 357
        }



        break;
    }
    return;
}

