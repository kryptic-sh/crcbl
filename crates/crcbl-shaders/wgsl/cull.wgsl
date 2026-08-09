struct CullParams_std140_0
{
    @align(16) planes_0 : array<vec4<f32>, i32(6)>,
    @align(16) instance_count_0 : u32,
    @align(4) capacity_0 : u32,
};

@binding(0) @group(0) var<uniform> cull_0 : CullParams_std140_0;
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

@binding(4) @group(0) var<storage, read_write> visible_count_0 : array<atomic<u32>>;

@binding(3) @group(0) var<storage, read_write> visible_0 : array<u32>;

fn abs_0( x_0 : mat3x3<f32>) -> mat3x3<f32>
{
    var result_0 : mat3x3<f32>;
    var i_0 : i32 = i32(0);
    for(;;)
    {
        if(i_0 < i32(3))
        {
        }
        else
        {
            break;
        }
        result_0[i_0] = abs(x_0[i_0]);
        i_0 = i_0 + i32(1);
    }
    return result_0;
}

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var index_0 : u32 = thread_0.x;
    if(index_0 >= (cull_0.instance_count_0))
    {
        return;
    }
    var instance_0 : GpuInstance_std430_0 = instances_0[index_0];
    var mesh_1 : GpuMesh_std430_0 = meshes_0[instance_0.mesh_0];
    if((mesh_1.index_count_0) == u32(0))
    {
        return;
    }
    var bounds_min_0 : vec3<f32> = vec3<f32>(mesh_1.min_x_0, mesh_1.min_y_0, mesh_1.min_z_0);
    var bounds_max_0 : vec3<f32> = vec3<f32>(mesh_1.max_x_0, mesh_1.max_y_0, mesh_1.max_z_0);
    var _S1 : vec3<f32> = vec3<f32>(0.5f);
    var _S2 : mat4x4<f32> = mat4x4<f32>(instance_0.transform_0.data_0[i32(0)][i32(0)], instance_0.transform_0.data_0[i32(1)][i32(0)], instance_0.transform_0.data_0[i32(2)][i32(0)], instance_0.transform_0.data_0[i32(3)][i32(0)], instance_0.transform_0.data_0[i32(0)][i32(1)], instance_0.transform_0.data_0[i32(1)][i32(1)], instance_0.transform_0.data_0[i32(2)][i32(1)], instance_0.transform_0.data_0[i32(3)][i32(1)], instance_0.transform_0.data_0[i32(0)][i32(2)], instance_0.transform_0.data_0[i32(1)][i32(2)], instance_0.transform_0.data_0[i32(2)][i32(2)], instance_0.transform_0.data_0[i32(3)][i32(2)], instance_0.transform_0.data_0[i32(0)][i32(3)], instance_0.transform_0.data_0[i32(1)][i32(3)], instance_0.transform_0.data_0[i32(2)][i32(3)], instance_0.transform_0.data_0[i32(3)][i32(3)]);
    var _S3 : vec3<f32> = (((vec4<f32>(_S1 * (bounds_max_0 + bounds_min_0), 1.0f)) * (_S2))).xyz;
    var _S4 : vec3<f32> = (((_S1 * (bounds_max_0 - bounds_min_0)) * (abs_0(mat3x3<f32>(_S2[i32(0)].xyz, _S2[i32(1)].xyz, _S2[i32(2)].xyz)))));
    var plane_0 : u32 = u32(0);
    for(;;)
    {
        if(plane_0 < u32(6))
        {
        }
        else
        {
            break;
        }
        var _S5 : vec3<f32> = cull_0.planes_0[plane_0].xyz;
        if((dot(_S5, _S3) + cull_0.planes_0[plane_0].w) < (- dot(abs(_S5), _S4)))
        {
            return;
        }
        plane_0 = plane_0 + u32(1);
    }
    var slot_0 : u32 = atomicAdd(&(visible_count_0[i32(0)]), u32(1));
    if(slot_0 < (cull_0.capacity_0))
    {
        visible_0[slot_0] = index_0;
    }
    return;
}

