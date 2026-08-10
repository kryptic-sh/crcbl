struct DrawGenParams_std140_0
{
    @align(16) bucket_count_0 : u32,
    @align(4) bucket_capacity_0 : u32,
    @align(8) visible_capacity_0 : u32,
    @align(4) pad0_0 : u32,
};

@binding(0) @group(0) var<uniform> gen_0 : DrawGenParams_std140_0;
@binding(5) @group(0) var<storage, read> bucket_meshes_0 : array<u32>;

struct GpuMesh_std430_0
{
    @align(4) base_vertex_0 : u32,
    @align(4) base_index_0 : u32,
    @align(4) index_count_0 : u32,
    @align(4) min_x_0 : f32,
    @align(4) min_y_0 : f32,
    @align(4) min_z_0 : f32,
    @align(4) max_x_0 : f32,
    @align(4) max_y_0 : f32,
    @align(4) max_z_0 : f32,
};

@binding(2) @group(0) var<storage, read> meshes_0 : array<GpuMesh_std430_0>;

@binding(7) @group(0) var<storage, read_write> args_0 : array<atomic<u32>>;

@binding(4) @group(0) var<storage, read> visible_count_0 : array<u32>;

@binding(3) @group(0) var<storage, read> visible_0 : array<u32>;

struct _MatrixStorage_float4x4_ColMajorstd430_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct GpuInstance_std430_0
{
    @align(16) transform_0 : _MatrixStorage_float4x4_ColMajorstd430_0,
    @align(16) mesh_0 : u32,
    @align(4) material_0 : u32,
    @align(8) sector_0 : u32,
    @align(4) flags_0 : u32,
};

@binding(1) @group(0) var<storage, read> instances_0 : array<GpuInstance_std430_0>;

@binding(6) @group(0) var<storage, read_write> visible_instances_0 : array<u32>;

@binding(8) @group(0) var<storage, read_write> draw_counts_0 : array<u32>;

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var index_0 : u32 = thread_0.x;
    if(index_0 < (gen_0.bucket_count_0))
    {
        var mesh_1 : GpuMesh_std430_0 = meshes_0[bucket_meshes_0[index_0]];
        var at_0 : u32 = index_0 * u32(5);
        atomicStore(&(args_0[at_0]), mesh_1.index_count_0);
        atomicStore(&(args_0[at_0 + u32(2)]), mesh_1.base_index_0);
        atomicStore(&(args_0[at_0 + u32(3)]), u32(0));
        atomicStore(&(args_0[at_0 + u32(4)]), u32(0));
    }
    if(index_0 >= (min(visible_count_0[i32(0)], min(gen_0.visible_capacity_0, gen_0.bucket_capacity_0))))
    {
        return;
    }
    var instance_index_0 : u32 = visible_0[index_0];
    var _S1 : GpuInstance_std430_0 = instances_0[instance_index_0];
    var bucket_0 : u32 = u32(0);
    for(;;)
    {
        if(bucket_0 < (gen_0.bucket_count_0))
        {
        }
        else
        {
            break;
        }
        if(bucket_meshes_0[bucket_0] != (_S1.mesh_0))
        {
            bucket_0 = bucket_0 + u32(1);
            continue;
        }
        var slot_0 : u32 = atomicAdd(&(args_0[bucket_0 * u32(5) + u32(1)]), u32(1));
        visible_instances_0[bucket_0 * gen_0.bucket_capacity_0 + slot_0] = instance_index_0;
        if(slot_0 == u32(0))
        {
            draw_counts_0[bucket_0] = u32(1);
        }
        break;
    }
    return;
}

