struct DrawGenParams_std140_0
{
    @align(16) bucket_count_0 : u32,
    @align(4) visible_capacity_0 : u32,
    @align(8) group_stride_0 : u32,
    @align(4) bucket_modes_at_0 : u32,
    @align(16) bucket_clusters_at_0 : u32,
    @align(4) mesh_levels_at_0 : u32,
    @align(8) level_groups_at_0 : u32,
    @align(4) level_meshes_at_0 : u32,
    @align(16) camera_position_0 : vec4<f32>,
    @align(16) lod_params_0 : vec4<f32>,
    @align(16) mode_0 : u32,
    @align(4) draw_regions_0 : u32,
    @align(8) face_runs_at_0 : u32,
    @align(4) mode_pad_0 : u32,
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
    @align(4) uv_scale_u_0 : f32,
    @align(4) uv_scale_v_0 : f32,
    @align(4) uv_offset_u_0 : f32,
    @align(4) uv_offset_v_0 : f32,
    @align(4) flags_0 : u32,
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
    @align(16) previous_transform_0 : _MatrixStorage_float4x4_ColMajorstd430_0,
    @align(16) mesh_0 : u32,
    @align(4) material_0 : u32,
    @align(8) sector_0 : u32,
    @align(4) flags_1 : u32,
    @align(16) base_vertex_1 : u32,
    @align(4) previous_base_vertex_0 : u32,
    @align(8) pad1_0 : u32,
    @align(4) pad2_0 : u32,
};

@binding(1) @group(0) var<storage, read> instances_0 : array<GpuInstance_std430_0>;

@binding(8) @group(0) var<storage, read_write> group_state_0 : array<u32>;

fn bucket_mesh_0( bucket_0 : u32) -> u32
{
    return tables_0[bucket_0];
}

fn draw_slot_0( region_0 : u32,  bucket_1 : u32) -> u32
{
    return region_0 * gen_0.bucket_count_0 + bucket_1;
}

fn arg_word_0( region_1 : u32,  bucket_2 : u32,  field_0 : u32) -> u32
{
    return draw_slot_0(region_1, bucket_2) * u32(5) + field_0;
}

fn mesh_arg_word_0( region_2 : u32,  bucket_3 : u32,  slot_0 : u32) -> u32
{
    return region_2 * u32(4) * gen_0.bucket_count_0 + gen_0.bucket_count_0 + bucket_3 * u32(3) + slot_0;
}

fn bucket_clusters_0( bucket_4 : u32) -> u32
{
    return tables_0[gen_0.bucket_clusters_at_0 + bucket_4];
}

fn survivor_count_0() -> u32
{
    return min(visible_count_0[u32(0)], gen_0.visible_capacity_0);
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

fn max_stretch_0( basis_0 : mat3x3<f32>) -> f32
{
    var _S1 : mat3x3<f32> = (((basis_0) * (transpose(basis_0))));
    var bound_0 : f32 = 0.0f;
    var row_0 : u32 = u32(0);
    for(;;)
    {
        if(row_0 < u32(3))
        {
        }
        else
        {
            break;
        }
        var _S2 : f32 = max(bound_0, abs(_S1[row_0][i32(0)]) + abs(_S1[row_0][i32(1)]) + abs(_S1[row_0][i32(2)]));
        var row_1 : u32 = row_0 + u32(1);
        bound_0 = _S2;
        row_0 = row_1;
    }
    return sqrt(bound_0);
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

fn projected_error_0( error_1 : f32,  center_0 : vec3<f32>,  radius_1 : f32,  eye_0 : vec3<f32>,  pixels_per_unit_0 : f32) -> f32
{
    var delta_0 : vec3<f32> = eye_0 - center_0;
    var _S3 : f32 = delta_0.x;
    var _S4 : f32 = delta_0.y;
    var _S5 : f32 = delta_0.z;
    var distance_0 : f32 = sqrt(_S3 * _S3 + _S4 * _S4 + _S5 * _S5) - radius_1;
    if(distance_0 <= 0.0f)
    {
        return 3.4028234663852886e+38f;
    }
    return error_1 * pixels_per_unit_0 / distance_0;
}

fn group_is_expanded_0( error_2 : f32,  center_1 : vec3<f32>,  radius_2 : f32,  eye_1 : vec3<f32>,  was_0 : u32) -> u32
{
    var projected_0 : f32 = projected_error_0(error_2, center_1, radius_2, eye_1, gen_0.lod_params_0.x);
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
    var _S6 : u32;
    if(expanded_0)
    {
        _S6 = u32(1);
    }
    else
    {
        _S6 = u32(0);
    }
    return _S6;
}

fn level_mesh_at_0( level_1 : u32) -> u32
{
    return tables_0[gen_0.level_meshes_at_0 + level_1];
}

fn bucket_mode_0( bucket_5 : u32) -> u32
{
    return tables_0[gen_0.bucket_modes_at_0 + bucket_5];
}

fn route_word_0( survivor_0 : u32) -> u32
{
    return gen_0.visible_capacity_0 + survivor_0;
}

fn select_level_0( _S7 : u32,  _S8 : u32) -> u32
{
    var levels_1 : MeshLevels_0 = mesh_levels_of_0(instances_0[_S7].mesh_0);
    var _S9 : vec3<f32> = gen_0.camera_position_0.xyz;
    var _S10 : mat4x4<f32> = mat4x4<f32>(instances_0[_S7].transform_0.data_0[i32(0)][i32(0)], instances_0[_S7].transform_0.data_0[i32(1)][i32(0)], instances_0[_S7].transform_0.data_0[i32(2)][i32(0)], instances_0[_S7].transform_0.data_0[i32(3)][i32(0)], instances_0[_S7].transform_0.data_0[i32(0)][i32(1)], instances_0[_S7].transform_0.data_0[i32(1)][i32(1)], instances_0[_S7].transform_0.data_0[i32(2)][i32(1)], instances_0[_S7].transform_0.data_0[i32(3)][i32(1)], instances_0[_S7].transform_0.data_0[i32(0)][i32(2)], instances_0[_S7].transform_0.data_0[i32(1)][i32(2)], instances_0[_S7].transform_0.data_0[i32(2)][i32(2)], instances_0[_S7].transform_0.data_0[i32(3)][i32(2)], instances_0[_S7].transform_0.data_0[i32(0)][i32(3)], instances_0[_S7].transform_0.data_0[i32(1)][i32(3)], instances_0[_S7].transform_0.data_0[i32(2)][i32(3)], instances_0[_S7].transform_0.data_0[i32(3)][i32(3)]);
    var _S11 : f32 = max_stretch_0(mat3x3<f32>(_S10[i32(0)].xyz, _S10[i32(1)].xyz, _S10[i32(2)].xyz));
    var _S12 : u32 = _S8 * gen_0.group_stride_0;
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
        var _S13 : u32 = _S12 + at_2;
        var expanded_1 : u32 = group_is_expanded_0(group_1.error_0 * _S11, (((vec4<f32>(group_1.center_x_0, group_1.center_y_0, group_1.center_z_0, 1.0f)) * (_S10))).xyz, group_1.radius_0 * _S11, _S9, group_state_0[_S13]);
        group_state_0[_S13] = expanded_1;
        var _S14 : bool;
        if(expanded_1 == u32(1))
        {
            _S14 = (group_1.level_0) < chosen_0;
        }
        else
        {
            _S14 = false;
        }
        if(_S14)
        {
            chosen_0 = group_1.level_0;
        }
        i_0 = i_0 + u32(1);
    }
    return chosen_0;
}

fn instance_material_mode_0( _S15 : u32) -> u32
{
    return ((((instances_0[_S15].flags_1) & (u32(12)))) >> (u32(2)));
}

@compute
@workgroup_size(64, 1, 1)
fn binMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var routed_0 : u32;
    var index_0 : u32 = thread_0.x;
    var bucket_6 : u32;
    if(index_0 < (gen_0.bucket_count_0))
    {
        var _S16 : GpuMesh_std430_0 = meshes_0[bucket_mesh_0(index_0)];
        bucket_6 = u32(0);
        for(;;)
        {
            if(bucket_6 < (gen_0.draw_regions_0))
            {
            }
            else
            {
                break;
            }
            atomicStore(&(args_0[arg_word_0(bucket_6, index_0, u32(0))]), _S16.index_count_0);
            atomicStore(&(args_0[arg_word_0(bucket_6, index_0, u32(2))]), _S16.base_index_0);
            atomicStore(&(args_0[arg_word_0(bucket_6, index_0, u32(3))]), u32(0));
            atomicStore(&(args_0[arg_word_0(bucket_6, index_0, u32(4))]), u32(0));
            atomicStore(&(counts_and_mesh_args_0[mesh_arg_word_0(bucket_6, index_0, u32(0))]), bucket_clusters_0(index_0));
            atomicStore(&(counts_and_mesh_args_0[mesh_arg_word_0(bucket_6, index_0, u32(2))]), u32(1));
            bucket_6 = bucket_6 + u32(1);
        }
    }
    if(index_0 >= (survivor_count_0()))
    {
        return;
    }
    var entry_0 : u32 = visible_instances_0[index_0];
    var instance_index_0 : u32 = (visible_instances_0[index_0] & (u32(16777215)));
    var _S17 : MeshLevels_0 = mesh_levels_of_0(instances_0[instance_index_0].mesh_0);
    var _S18 : u32 = select_level_0(instance_index_0, instance_index_0);
    var _S19 : u32 = level_mesh_at_0(_S17.first_level_0 + _S18);
    var _S20 : u32 = instance_material_mode_0(instance_index_0);
    bucket_6 = u32(0);
    for(;;)
    {
        if(bucket_6 < (gen_0.bucket_count_0))
        {
        }
        else
        {
            routed_0 = u32(4294967295);
            break;
        }
        var _S21 : bool;
        if((bucket_mesh_0(bucket_6)) != _S19)
        {
            _S21 = true;
        }
        else
        {
            _S21 = (bucket_mode_0(bucket_6)) != _S20;
        }
        if(_S21)
        {
            bucket_6 = bucket_6 + u32(1);
            continue;
        }
        if((gen_0.mode_0) == u32(2))
        {
            routed_0 = u32(0);
            for(;;)
            {
                if(routed_0 < u32(6))
                {
                }
                else
                {
                    break;
                }
                if(((((entry_0 >> ((u32(24) + routed_0)))) & (u32(1)))) != u32(0))
                {
                    var _S22 : u32 = atomicAdd(&(counts_and_mesh_args_0[mesh_arg_word_0(u32(1) + routed_0, bucket_6, u32(1))]), u32(1));
                }
                routed_0 = routed_0 + u32(1);
            }
        }
        else
        {
            var _S23 : u32 = atomicAdd(&(counts_and_mesh_args_0[mesh_arg_word_0(u32(0), bucket_6, u32(1))]), u32(1));
            if((gen_0.mode_0) == u32(1))
            {
                _S21 = ((entry_0 & (u32(2147483648)))) == u32(0);
            }
            else
            {
                _S21 = false;
            }
            if(_S21)
            {
                var _S24 : u32 = atomicAdd(&(counts_and_mesh_args_0[mesh_arg_word_0(u32(1), bucket_6, u32(1))]), u32(1));
            }
        }
        routed_0 = bucket_6;
        break;
    }
    visible_instances_0[route_word_0(index_0)] = routed_0;
    return;
}

fn run_start_word_0( region_3 : u32,  bucket_7 : u32) -> u32
{
    return u32(3) * gen_0.visible_capacity_0 + draw_slot_0(region_3, bucket_7);
}

fn count_word_0( region_4 : u32,  bucket_8 : u32) -> u32
{
    return region_4 * u32(4) * gen_0.bucket_count_0 + bucket_8;
}

fn runs_at_0() -> u32
{
    return u32(2) * gen_0.visible_capacity_0;
}

@compute
@workgroup_size(1, 1, 1)
fn startsMain()
{
    var face_0 : u32;
    var face_start_0 : u32;
    if((gen_0.mode_0) == u32(2))
    {
        face_0 = u32(0);
        face_start_0 = gen_0.face_runs_at_0;
        for(;;)
        {
            if(face_0 < u32(6))
            {
            }
            else
            {
                break;
            }
            var _S25 : u32 = u32(1) + face_0;
            var bucket_9 : u32 = u32(0);
            for(;;)
            {
                if(bucket_9 < (gen_0.bucket_count_0))
                {
                }
                else
                {
                    break;
                }
                visible_instances_0[run_start_word_0(_S25, bucket_9)] = face_start_0;
                var reached_0 : u32 = atomicLoad(&(counts_and_mesh_args_0[mesh_arg_word_0(_S25, bucket_9, u32(1))]));
                if(reached_0 != u32(0))
                {
                    atomicStore(&(counts_and_mesh_args_0[count_word_0(_S25, bucket_9)]), u32(1));
                }
                var face_start_1 : u32 = face_start_0 + reached_0;
                bucket_9 = bucket_9 + u32(1);
                face_start_0 = face_start_1;
            }
            face_0 = face_0 + u32(1);
        }
        return;
    }
    var _S26 : u32 = runs_at_0();
    face_0 = u32(0);
    face_start_0 = _S26;
    for(;;)
    {
        if(face_0 < (gen_0.bucket_count_0))
        {
        }
        else
        {
            break;
        }
        visible_instances_0[run_start_word_0(u32(0), face_0)] = face_start_0;
        var routed_1 : u32 = atomicLoad(&(counts_and_mesh_args_0[mesh_arg_word_0(u32(0), face_0, u32(1))]));
        if(routed_1 != u32(0))
        {
            atomicStore(&(counts_and_mesh_args_0[count_word_0(u32(0), face_0)]), u32(1));
        }
        if((gen_0.mode_0) == u32(1))
        {
            var early_0 : u32 = atomicLoad(&(counts_and_mesh_args_0[mesh_arg_word_0(u32(1), face_0, u32(1))]));
            visible_instances_0[run_start_word_0(u32(1), face_0)] = face_start_0;
            visible_instances_0[run_start_word_0(u32(2), face_0)] = face_start_0 + early_0;
            if(early_0 != u32(0))
            {
                atomicStore(&(counts_and_mesh_args_0[count_word_0(u32(1), face_0)]), u32(1));
            }
        }
        var start_0 : u32 = face_start_0 + routed_1;
        face_0 = face_0 + u32(1);
        face_start_0 = start_0;
    }
    return;
}

@compute
@workgroup_size(64, 1, 1)
fn scatterMain(@builtin(global_invocation_id) thread_1 : vec3<u32>)
{
    var index_1 : u32 = thread_1.x;
    if(index_1 >= (survivor_count_0()))
    {
        return;
    }
    var bucket_10 : u32 = visible_instances_0[route_word_0(index_1)];
    if(visible_instances_0[route_word_0(index_1)] == u32(4294967295))
    {
        return;
    }
    var entry_1 : u32 = visible_instances_0[index_1];
    var instance_index_1 : u32 = (visible_instances_0[index_1] & (u32(16777215)));
    var face_1 : u32;
    if((gen_0.mode_0) == u32(2))
    {
        face_1 = u32(0);
        for(;;)
        {
            if(face_1 < u32(6))
            {
            }
            else
            {
                break;
            }
            if(((((entry_1 >> ((u32(24) + face_1)))) & (u32(1)))) == u32(0))
            {
                face_1 = face_1 + u32(1);
                continue;
            }
            var region_5 : u32 = u32(1) + face_1;
            var face_slot_0 : u32 = atomicAdd(&(args_0[arg_word_0(region_5, bucket_10, u32(1))]), u32(1));
            visible_instances_0[visible_instances_0[run_start_word_0(region_5, bucket_10)] + face_slot_0] = instance_index_1;
            face_1 = face_1 + u32(1);
        }
        return;
    }
    var _S27 : bool;
    if((gen_0.mode_0) == u32(1))
    {
        _S27 = ((entry_1 & (u32(2147483648)))) != u32(0);
    }
    else
    {
        _S27 = false;
    }
    if(_S27)
    {
        return;
    }
    if((gen_0.mode_0) == u32(1))
    {
        face_1 = u32(1);
    }
    else
    {
        face_1 = u32(0);
    }
    var slot_1 : u32 = atomicAdd(&(args_0[arg_word_0(face_1, bucket_10, u32(1))]), u32(1));
    visible_instances_0[visible_instances_0[run_start_word_0(face_1, bucket_10)] + slot_1] = instance_index_1;
    return;
}

@compute
@workgroup_size(64, 1, 1)
fn lateScatterMain(@builtin(global_invocation_id) thread_2 : vec3<u32>)
{
    var index_2 : u32 = thread_2.x;
    if(index_2 >= (survivor_count_0()))
    {
        return;
    }
    var entry_2 : u32 = visible_instances_0[index_2];
    if(((visible_instances_0[index_2] & (u32(1073741824)))) == u32(0))
    {
        return;
    }
    var bucket_11 : u32 = visible_instances_0[route_word_0(index_2)];
    if(visible_instances_0[route_word_0(index_2)] == u32(4294967295))
    {
        return;
    }
    var slot_2 : u32 = atomicAdd(&(args_0[arg_word_0(u32(2), bucket_11, u32(1))]), u32(1));
    visible_instances_0[visible_instances_0[run_start_word_0(u32(2), bucket_11)] + slot_2] = (entry_2 & (u32(16777215)));
    return;
}

@compute
@workgroup_size(1, 1, 1)
fn lateFinishMain()
{
    var bucket_12 : u32 = u32(0);
    for(;;)
    {
        if(bucket_12 < (gen_0.bucket_count_0))
        {
        }
        else
        {
            break;
        }
        var early_1 : u32 = atomicLoad(&(args_0[arg_word_0(u32(1), bucket_12, u32(1))]));
        var late_0 : u32 = atomicLoad(&(args_0[arg_word_0(u32(2), bucket_12, u32(1))]));
        atomicStore(&(counts_and_mesh_args_0[mesh_arg_word_0(u32(2), bucket_12, u32(1))]), late_0);
        if(late_0 != u32(0))
        {
            atomicStore(&(counts_and_mesh_args_0[count_word_0(u32(2), bucket_12)]), u32(1));
        }
        var drawn_0 : u32 = early_1 + late_0;
        atomicStore(&(args_0[arg_word_0(u32(0), bucket_12, u32(1))]), drawn_0);
        atomicStore(&(counts_and_mesh_args_0[mesh_arg_word_0(u32(0), bucket_12, u32(1))]), drawn_0);
        var _S28 : i32;
        if(drawn_0 != u32(0))
        {
            _S28 = i32(1);
        }
        else
        {
            _S28 = i32(0);
        }
        atomicStore(&(counts_and_mesh_args_0[count_word_0(u32(0), bucket_12)]), u32(_S28));
        bucket_12 = bucket_12 + u32(1);
    }
    return;
}

