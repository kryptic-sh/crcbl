struct DrawGenParams_std140_0
{
    @align(16) bucket_count_0 : u32,
    @align(4) bucket_capacity_0 : u32,
    @align(8) visible_capacity_0 : u32,
    @align(4) group_stride_0 : u32,
    @align(16) bucket_clusters_at_0 : u32,
    @align(4) mesh_levels_at_0 : u32,
    @align(8) level_groups_at_0 : u32,
    @align(4) level_meshes_at_0 : u32,
    @align(16) camera_position_0 : vec4<f32>,
    @align(16) lod_params_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> gen_0 : DrawGenParams_std140_0;
@binding(4) @group(0) var<storage, read> tables_0 : array<u32>;

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

@binding(6) @group(0) var<storage, read_write> args_0 : array<atomic<u32>>;

@binding(7) @group(0) var<storage, read_write> counts_and_mesh_args_0 : array<atomic<u32>>;

@binding(3) @group(0) var<storage, read> visible_count_0 : array<u32>;

@binding(5) @group(0) var<storage, read_write> visible_instances_0 : array<u32>;

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

@binding(8) @group(0) var<storage, read_write> group_state_0 : array<u32>;

fn bucket_mesh_0( bucket_0 : u32) -> u32
{
    return tables_0[bucket_0];
}

fn mesh_arg_word_0( bucket_1 : u32,  slot_0 : u32) -> u32
{
    return gen_0.bucket_count_0 + bucket_1 * u32(3) + slot_0;
}

fn bucket_clusters_0( bucket_2 : u32) -> u32
{
    return tables_0[gen_0.bucket_clusters_at_0 + bucket_2];
}

struct MeshLevels_0
{
     first_group_0 : u32,
     group_count_0 : u32,
     first_level_0 : u32,
     top_level_0 : u32,
};

fn mesh_levels_of_0( mesh_1 : u32) -> MeshLevels_0
{
    var at_0 : u32 = gen_0.mesh_levels_at_0 + mesh_1 * u32(4);
    var levels_0 : MeshLevels_0;
    levels_0.first_group_0 = tables_0[at_0];
    levels_0.group_count_0 = tables_0[at_0 + u32(1)];
    levels_0.first_level_0 = tables_0[at_0 + u32(2)];
    levels_0.top_level_0 = tables_0[at_0 + u32(3)];
    return levels_0;
}

struct LevelGroup_0
{
     level_0 : u32,
     error_0 : f32,
     center_x_0 : f32,
     center_y_0 : f32,
     center_z_0 : f32,
     radius_0 : f32,
};

fn level_group_at_0( group_0 : u32) -> LevelGroup_0
{
    var at_1 : u32 = gen_0.level_groups_at_0 + group_0 * u32(6);
    var record_0 : LevelGroup_0;
    record_0.level_0 = tables_0[at_1];
    record_0.error_0 = (bitcast<f32>((tables_0[at_1 + u32(1)])));
    record_0.center_x_0 = (bitcast<f32>((tables_0[at_1 + u32(2)])));
    record_0.center_y_0 = (bitcast<f32>((tables_0[at_1 + u32(3)])));
    record_0.center_z_0 = (bitcast<f32>((tables_0[at_1 + u32(4)])));
    record_0.radius_0 = (bitcast<f32>((tables_0[at_1 + u32(5)])));
    return record_0;
}

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
    var levels_1 : MeshLevels_0 = mesh_levels_of_0((*instance_0).mesh_0);
    var _S5 : vec3<f32> = gen_0.camera_position_0.xyz;
    var _S6 : u32 = instance_index_0 * gen_0.group_stride_0;
    var chosen_0 : u32 = levels_1.top_level_0;
    var i_0 : u32 = u32(0);
    for(;;)
    {
        if(i_0 < (levels_1.group_count_0))
        {
        }
        else
        {
            break;
        }
        var at_2 : u32 = levels_1.first_group_0 + i_0;
        var group_1 : LevelGroup_0 = level_group_at_0(at_2);
        var _S7 : u32 = _S6 + at_2;
        var expanded_1 : u32 = group_is_expanded_0(group_1.error_0, (((vec4<f32>(group_1.center_x_0, group_1.center_y_0, group_1.center_z_0, 1.0f)) * (mat4x4<f32>((*instance_0).transform_0.data_0[i32(0)][i32(0)], (*instance_0).transform_0.data_0[i32(1)][i32(0)], (*instance_0).transform_0.data_0[i32(2)][i32(0)], (*instance_0).transform_0.data_0[i32(3)][i32(0)], (*instance_0).transform_0.data_0[i32(0)][i32(1)], (*instance_0).transform_0.data_0[i32(1)][i32(1)], (*instance_0).transform_0.data_0[i32(2)][i32(1)], (*instance_0).transform_0.data_0[i32(3)][i32(1)], (*instance_0).transform_0.data_0[i32(0)][i32(2)], (*instance_0).transform_0.data_0[i32(1)][i32(2)], (*instance_0).transform_0.data_0[i32(2)][i32(2)], (*instance_0).transform_0.data_0[i32(3)][i32(2)], (*instance_0).transform_0.data_0[i32(0)][i32(3)], (*instance_0).transform_0.data_0[i32(1)][i32(3)], (*instance_0).transform_0.data_0[i32(2)][i32(3)], (*instance_0).transform_0.data_0[i32(3)][i32(3)])))).xyz, group_1.radius_0, _S5, group_state_0[_S7]);
        group_state_0[_S7] = expanded_1;
        var _S8 : bool;
        if(expanded_1 == u32(1))
        {
            _S8 = (group_1.level_0) < chosen_0;
        }
        else
        {
            _S8 = false;
        }
        if(_S8)
        {
            chosen_0 = group_1.level_0;
        }
        i_0 = i_0 + u32(1);
    }
    return chosen_0;
}

fn level_mesh_at_0( level_1 : u32) -> u32
{
    return tables_0[gen_0.level_meshes_at_0 + level_1];
}

fn bucket_base_0( bucket_3 : u32) -> u32
{
    return gen_0.visible_capacity_0 + bucket_3 * gen_0.bucket_capacity_0;
}

fn count_word_0( bucket_4 : u32) -> u32
{
    return bucket_4;
}

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var index_0 : u32 = thread_0.x;
    if(index_0 < (gen_0.bucket_count_0))
    {
        var mesh_2 : GpuMesh_std430_0 = meshes_0[bucket_mesh_0(index_0)];
        var at_3 : u32 = index_0 * u32(5);
        atomicStore(&(args_0[at_3]), mesh_2.index_count_0);
        atomicStore(&(args_0[at_3 + u32(2)]), mesh_2.base_index_0);
        atomicStore(&(args_0[at_3 + u32(3)]), u32(0));
        atomicStore(&(args_0[at_3 + u32(4)]), u32(0));
        atomicStore(&(counts_and_mesh_args_0[mesh_arg_word_0(index_0, u32(0))]), bucket_clusters_0(index_0));
        atomicStore(&(counts_and_mesh_args_0[mesh_arg_word_0(index_0, u32(2))]), u32(1));
    }
    if(index_0 >= (min(visible_count_0[u32(0)], min(gen_0.visible_capacity_0, gen_0.bucket_capacity_0))))
    {
        return;
    }
    var instance_index_1 : u32 = visible_instances_0[index_0];
    var _S9 : GpuInstance_std430_0 = instances_0[visible_instances_0[index_0]];
    var _S10 : MeshLevels_0 = mesh_levels_of_0(_S9.mesh_0);
    var _S11 : u32 = select_level_0(&(_S9), visible_instances_0[index_0]);
    var _S12 : u32 = level_mesh_at_0(_S10.first_level_0 + _S11);
    var bucket_5 : u32 = u32(0);
    for(;;)
    {
        if(bucket_5 < (gen_0.bucket_count_0))
        {
        }
        else
        {
            break;
        }
        if((bucket_mesh_0(bucket_5)) != _S12)
        {
            bucket_5 = bucket_5 + u32(1);
            continue;
        }
        var slot_1 : u32 = atomicAdd(&(args_0[bucket_5 * u32(5) + u32(1)]), u32(1));
        var _S13 : u32 = atomicAdd(&(counts_and_mesh_args_0[mesh_arg_word_0(bucket_5, u32(1))]), u32(1));
        visible_instances_0[bucket_base_0(bucket_5) + slot_1] = instance_index_1;
        if(slot_1 == u32(0))
        {
            atomicStore(&(counts_and_mesh_args_0[count_word_0(bucket_5)]), u32(1));
        }
        break;
    }
    return;
}

