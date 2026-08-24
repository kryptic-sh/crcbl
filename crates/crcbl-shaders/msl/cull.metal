#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 7508 "hlsl.meta.slang"
matrix<float,int(3),int(3)>  abs_0(matrix<float,int(3),int(3)>  x_0)
{

#line 7386
    thread matrix<float,int(3),int(3)>  result_0;

#line 7386
    int i_0 = int(0);

#line 7386
    for(;;)
    {

#line 7386
        if(i_0 < int(3))
        {
        }
        else
        {

#line 7386
            break;
        }

#line 7386
        result_0[i_0] = abs(x_0[i_0]);

#line 7386
        i_0 = i_0 + int(1);

#line 7386
    }

#line 7386
    return result_0;
}


#line 154 "shaders/cull.slang"
struct CullParams_0
{
    array<float4, int(6)> planes_0;
    uint instance_count_0;
    uint capacity_0;
};


#line 154
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 154
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
    uint base_vertex_0;
    uint pad0_0;
    uint pad1_0;
    uint pad2_0;
};


#line 123
struct GpuMesh_0
{
    uint base_vertex_1;
    uint base_index_0;
    uint index_count_0;
    float min_x_0;
    float min_y_0;
    float min_z_0;
    float max_x_0;
    float max_y_0;
    float max_z_0;
};


#line 278
struct KernelContext_0
{
    CullParams_0 constant* cull_0;
    GpuInstance_natural_0 device* instances_0;
    GpuMesh_0 device* meshes_0;
    atomic<uint> device* visible_count_0;
    uint device* visible_0;
};


#line 218
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], CullParams_0 constant* cull_1 [[buffer(0)]], GpuInstance_natural_0 device* instances_1 [[buffer(1)]], GpuMesh_0 device* meshes_1 [[buffer(2)]], atomic<uint> device* visible_count_1 [[buffer(4)]], uint device* visible_1 [[buffer(3)]])
{

#line 218
    thread KernelContext_0 kernelContext_0;

#line 218
    (&kernelContext_0)->cull_0 = cull_1;

#line 218
    (&kernelContext_0)->instances_0 = instances_1;

#line 218
    (&kernelContext_0)->meshes_0 = meshes_1;

#line 218
    (&kernelContext_0)->visible_count_0 = visible_count_1;

#line 218
    (&kernelContext_0)->visible_0 = visible_1;

    uint index_0 = thread_0.x;
    if(index_0 >= (cull_1->instance_count_0))
    {
        return;
    }

    GpuInstance_natural_0 instance_0 = (&kernelContext_0)->instances_0[index_0];

#line 233
    if(((instance_0.flags_0) & 1U) == 0U)
    {
        return;
    }

    GpuMesh_0 mesh_1 = (&kernelContext_0)->meshes_0[instance_0.mesh_0];

#line 244
    if((mesh_1.index_count_0) == 0U)
    {
        return;
    }

    float3 bounds_min_0 = float3(mesh_1.min_x_0, mesh_1.min_y_0, mesh_1.min_z_0);
    float3 bounds_max_0 = float3(mesh_1.max_x_0, mesh_1.max_y_0, mesh_1.max_z_0);

#line 250
    float3 _S1 = float3(0.5f) ;

#line 250
    matrix<float,int(4),int(4)>  _S2 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

#line 259
    float3 _S3 = (((float4(_S1 * (bounds_max_0 + bounds_min_0), 1.0f)) * (_S2))).xyz;
    float3 _S4 = (((_S1 * (bounds_max_0 - bounds_min_0)) * (abs_0(matrix<float,int(3),int(3)> (_S2[int(0)].xyz, _S2[int(1)].xyz, _S2[int(2)].xyz)))));

#line 260
    uint plane_0 = 0U;

    for(;;)
    {

#line 262
        if(plane_0 < 6U)
        {
        }
        else
        {

#line 262
            break;
        }

#line 268
        float3 _S5 = (&kernelContext_0)->cull_0->planes_0[plane_0].xyz;
        if((dot(_S5, _S3) + (&kernelContext_0)->cull_0->planes_0[plane_0].w) < (- dot(abs(_S5), _S4)))
        {
            return;
        }

#line 262
        plane_0 = plane_0 + 1U;

#line 262
    }

#line 275
    uint slot_0 = atomic_fetch_add_explicit((&kernelContext_0)->visible_count_0+0U, 1U, memory_order_relaxed);
    if(slot_0 < ((&kernelContext_0)->cull_0->capacity_0))
    {
        *((&kernelContext_0)->visible_0+slot_0) = index_0;

#line 276
    }



    return;
}

