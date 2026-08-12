struct DrawGenParams_std140_0
{
    @align(16) bucket_count_0 : u32,
    @align(4) bucket_capacity_0 : u32,
    @align(8) visible_capacity_0 : u32,
    @align(4) group_stride_0 : u32,
    @align(16) camera_position_0 : vec4<f32>,
    @align(16) lod_params_0 : vec4<f32>,
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

@binding(10) @group(0) var<storage, read_write> mesh_args_0 : array<atomic<u32>>;

@binding(9) @group(0) var<storage, read> bucket_clusters_0 : array<u32>;

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

struct MeshLevels_std430_0
{
    @align(4) first_group_0 : u32,
    @align(4) group_count_0 : u32,
    @align(4) first_level_0 : u32,
    @align(4) top_level_0 : u32,
};

@binding(11) @group(0) var<storage, read> mesh_levels_0 : array<MeshLevels_std430_0>;

struct LevelGroup_std430_0
{
    @align(4) level_0 : u32,
    @align(4) error_0 : f32,
    @align(4) center_x_0 : f32,
    @align(4) center_y_0 : f32,
    @align(4) center_z_0 : f32,
    @align(4) radius_0 : f32,
};

@binding(12) @group(0) var<storage, read> level_groups_0 : array<LevelGroup_std430_0>;

@binding(14) @group(0) var<storage, read_write> group_state_0 : array<u32>;

@binding(13) @group(0) var<storage, read> level_meshes_0 : array<u32>;

@binding(6) @group(0) var<storage, read_write> visible_instances_0 : array<u32>;

@binding(8) @group(0) var<storage, read_write> draw_counts_0 : array<u32>;

fn group_is_expanded_0( error_1 : f32,  center_0 : vec3<f32>,  radius_1 : f32,  eye_0 : vec3<f32>,  was_0 : u32) -> u32
{
    var delta_0 : vec3<f32> = eye_0 - center_0;
    var _S1 : f32 = delta_0.x;
    var _S2 : f32 = delta_0.y;
    var _S3 : f32 = delta_0.z;
    var distance_0 : f32 = sqrt(_S1 * _S1 + _S2 * _S2 + _S3 * _S3) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return u32(1);
    }
    var projected_0 : f32 = error_1 * gen_0.lod_params_0.x / distance_0;
    var expanded_0 : bool;
    if(projected_0 > (gen_0.lod_params_0.y))
    {
        expanded_0 = true;
    }
    else
    {
        if(was_0 != u32(0))
        {
            expanded_0 = projected_0 > (gen_0.lod_params_0.z);
        }
        else
        {
            expanded_0 = false;
        }
    }
    var _S4 : u32;
    if(expanded_0)
    {
        _S4 = u32(1);
    }
    else
    {
        _S4 = u32(0);
    }
    return _S4;
}

fn select_level_0( instance_0 : ptr<function, GpuInstance_std430_0>,  instance_index_0 : u32) -> u32
{
    var levels_0 : MeshLevels_std430_0 = mesh_levels_0[(*instance_0).mesh_0];
    var _S5 : vec3<f32> = gen_0.camera_position_0.xyz;
    var _S6 : u32 = instance_index_0 * gen_0.group_stride_0;
    var chosen_0 : u32 = levels_0.top_level_0;
    var i_0 : u32 = u32(0);
    for(;;)
    {
        if(i_0 < (levels_0.group_count_0))
        {
        }
        else
        {
            break;
        }
        var at_0 : u32 = levels_0.first_group_0 + i_0;
        var group_0 : LevelGroup_std430_0 = level_groups_0[at_0];
        var _S7 : u32 = _S6 + at_0;
        var expanded_1 : u32 = group_is_expanded_0(group_0.error_0, (((vec4<f32>(group_0.center_x_0, group_0.center_y_0, group_0.center_z_0, 1.0f)) * (mat4x4<f32>((*instance_0).transform_0.data_0[i32(0)][i32(0)], (*instance_0).transform_0.data_0[i32(1)][i32(0)], (*instance_0).transform_0.data_0[i32(2)][i32(0)], (*instance_0).transform_0.data_0[i32(3)][i32(0)], (*instance_0).transform_0.data_0[i32(0)][i32(1)], (*instance_0).transform_0.data_0[i32(1)][i32(1)], (*instance_0).transform_0.data_0[i32(2)][i32(1)], (*instance_0).transform_0.data_0[i32(3)][i32(1)], (*instance_0).transform_0.data_0[i32(0)][i32(2)], (*instance_0).transform_0.data_0[i32(1)][i32(2)], (*instance_0).transform_0.data_0[i32(2)][i32(2)], (*instance_0).transform_0.data_0[i32(3)][i32(2)], (*instance_0).transform_0.data_0[i32(0)][i32(3)], (*instance_0).transform_0.data_0[i32(1)][i32(3)], (*instance_0).transform_0.data_0[i32(2)][i32(3)], (*instance_0).transform_0.data_0[i32(3)][i32(3)])))).xyz, group_0.radius_0, _S5, group_state_0[_S7]);
        group_state_0[_S7] = expanded_1;
        var _S8 : bool;
        if(expanded_1 == u32(1))
        {
            _S8 = (group_0.level_0) < chosen_0;
        }
        else
        {
            _S8 = false;
        }
        if(_S8)
        {
            chosen_0 = group_0.level_0;
        }
        i_0 = i_0 + u32(1);
    }
    return chosen_0;
}

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var index_0 : u32 = thread_0.x;
    if(index_0 < (gen_0.bucket_count_0))
    {
        var mesh_1 : GpuMesh_std430_0 = meshes_0[bucket_meshes_0[index_0]];
        var at_1 : u32 = index_0 * u32(5);
        atomicStore(&(args_0[at_1]), mesh_1.index_count_0);
        atomicStore(&(args_0[at_1 + u32(2)]), mesh_1.base_index_0);
        atomicStore(&(args_0[at_1 + u32(3)]), u32(0));
        atomicStore(&(args_0[at_1 + u32(4)]), u32(0));
        var mesh_at_0 : u32 = index_0 * u32(3);
        atomicStore(&(mesh_args_0[mesh_at_0]), bucket_clusters_0[index_0]);
        atomicStore(&(mesh_args_0[mesh_at_0 + u32(2)]), u32(1));
    }
    if(index_0 >= (min(visible_count_0[i32(0)], min(gen_0.visible_capacity_0, gen_0.bucket_capacity_0))))
    {
        return;
    }
    var instance_index_1 : u32 = visible_0[index_0];
    var _S9 : GpuInstance_std430_0 = instances_0[instance_index_1];
    var _S10 : MeshLevels_std430_0 = mesh_levels_0[_S9.mesh_0];
    var _S11 : u32 = select_level_0(&(_S9), instance_index_1);
    var _S12 : u32 = level_meshes_0[_S10.first_level_0 + _S11];
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
        if(bucket_meshes_0[bucket_0] != _S12)
        {
            bucket_0 = bucket_0 + u32(1);
            continue;
        }
        var slot_0 : u32 = atomicAdd(&(args_0[bucket_0 * u32(5) + u32(1)]), u32(1));
        var _S13 : u32 = atomicAdd(&(mesh_args_0[bucket_0 * u32(3) + u32(1)]), u32(1));
        visible_instances_0[bucket_0 * gen_0.bucket_capacity_0 + slot_0] = instance_index_1;
        if(slot_0 == u32(0))
        {
            draw_counts_0[bucket_0] = u32(1);
        }
        break;
    }
    return;
}

