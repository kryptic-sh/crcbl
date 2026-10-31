struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct CullParams_std140_0
{
    @align(16) planes_0 : array<vec4<f32>, i32(6)>,
    @align(16) instance_count_0 : u32,
    @align(4) capacity_0 : u32,
    @align(8) hidden_view_0 : u32,
    @align(4) features_0 : u32,
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) previous_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) target_width_0 : u32,
    @align(4) target_height_0 : u32,
    @align(8) pyramid_levels_0 : u32,
    @align(4) occlusion_pad_0 : u32,
    @align(16) small_feature_pixels_0 : f32,
    @align(4) small_feature_pad0_0 : f32,
    @align(8) small_feature_pad1_0 : f32,
    @align(4) small_feature_pad2_0 : f32,
    @align(16) face_planes_0 : array<vec4<f32>, i32(24)>,
};

@binding(0) @group(0) var<uniform> cull_0 : CullParams_std140_0;
struct _MatrixStorage_float4x4_ColMajorstd430_0
{
    @align(16) data_1 : array<vec4<f32>, i32(4)>,
};

struct GpuInstance_std430_0
{
    @align(16) transform_0 : _MatrixStorage_float4x4_ColMajorstd430_0,
    @align(16) previous_transform_0 : _MatrixStorage_float4x4_ColMajorstd430_0,
    @align(16) mesh_0 : u32,
    @align(4) material_0 : u32,
    @align(8) sector_0 : u32,
    @align(4) flags_0 : u32,
    @align(16) base_vertex_0 : u32,
    @align(4) previous_base_vertex_0 : u32,
    @align(8) bounds_min_x_0 : f32,
    @align(4) bounds_min_y_0 : f32,
    @align(16) bounds_min_z_0 : f32,
    @align(4) bounds_max_x_0 : f32,
    @align(8) bounds_max_y_0 : f32,
    @align(4) bounds_max_z_0 : f32,
};

@binding(1) @group(0) var<storage, read> instances_0 : array<GpuInstance_std430_0>;

struct GpuMesh_std430_0
{
    @align(4) base_vertex_1 : u32,
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
    @align(4) flags_1 : u32,
};

@binding(2) @group(0) var<storage, read> meshes_0 : array<GpuMesh_std430_0>;

@binding(4) @group(0) var<storage, read_write> visible_count_0 : array<atomic<u32>>;

@binding(3) @group(0) var<storage, read_write> visible_0 : array<u32>;

@binding(0) @group(1) var pyramid_1_0 : texture_depth_2d;

@binding(1) @group(1) var pyramid_2_0 : texture_depth_2d;

@binding(2) @group(1) var pyramid_3_0 : texture_depth_2d;

@binding(3) @group(1) var pyramid_4_0 : texture_depth_2d;

@binding(4) @group(1) var pyramid_5_0 : texture_depth_2d;

@binding(5) @group(1) var pyramid_6_0 : texture_depth_2d;

@binding(6) @group(1) var pyramid_7_0 : texture_depth_2d;

@binding(7) @group(1) var pyramid_8_0 : texture_depth_2d;

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

fn reaches_0( half_space_0 : vec4<f32>,  center_0 : vec3<f32>,  extent_0 : vec3<f32>) -> bool
{
    var _S1 : vec3<f32> = half_space_0.xyz;
    return (dot(_S1, center_0) + half_space_0.w) >= (- dot(abs(_S1), extent_0));
}

fn in_frustum_0( center_1 : vec3<f32>,  extent_1 : vec3<f32>) -> bool
{
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
        if(!reaches_0(cull_0.planes_0[plane_0], center_1, extent_1))
        {
            return false;
        }
        plane_0 = plane_0 + u32(1);
    }
    return true;
}

fn admits_0( _S2 : u32) -> bool
{
    var _S3 : u32 = instances_0[_S2].flags_0;
    if((((instances_0[_S2].flags_0) & (u32(1)))) == u32(0))
    {
        return false;
    }
    return ((_S3 & ((cull_0.hidden_view_0)))) == u32(0);
}

fn has_box_0( _S4 : u32) -> bool
{
    var _S5 : u32 = instances_0[_S4].flags_0;
    var _S6 : bool;
    if((((instances_0[_S4].flags_0) & (u32(131072)))) != u32(0))
    {
        _S6 = true;
    }
    else
    {
        _S6 = ((_S5 & (u32(2)))) == u32(0);
    }
    return _S6;
}

fn world_box_0( _S7 : u32,  _S8 : ptr<function, GpuMesh_std430_0>,  _S9 : ptr<function, vec3<f32>>,  _S10 : ptr<function, vec3<f32>>)
{
    var owns_0 : bool = (((instances_0[_S7].flags_0) & (u32(131072)))) != u32(0);
    var bounds_min_0 : vec3<f32>;
    if(owns_0)
    {
        bounds_min_0 = vec3<f32>(instances_0[_S7].bounds_min_x_0, instances_0[_S7].bounds_min_y_0, instances_0[_S7].bounds_min_z_0);
    }
    else
    {
        bounds_min_0 = vec3<f32>((*_S8).min_x_0, (*_S8).min_y_0, (*_S8).min_z_0);
    }
    var bounds_max_0 : vec3<f32>;
    if(owns_0)
    {
        bounds_max_0 = vec3<f32>(instances_0[_S7].bounds_max_x_0, instances_0[_S7].bounds_max_y_0, instances_0[_S7].bounds_max_z_0);
    }
    else
    {
        bounds_max_0 = vec3<f32>((*_S8).max_x_0, (*_S8).max_y_0, (*_S8).max_z_0);
    }
    var _S11 : vec3<f32> = vec3<f32>(0.5f);
    var local_extent_0 : vec3<f32> = _S11 * (bounds_max_0 - bounds_min_0);
    var _S12 : mat4x4<f32> = mat4x4<f32>(instances_0[_S7].transform_0.data_1[i32(0)][i32(0)], instances_0[_S7].transform_0.data_1[i32(1)][i32(0)], instances_0[_S7].transform_0.data_1[i32(2)][i32(0)], instances_0[_S7].transform_0.data_1[i32(3)][i32(0)], instances_0[_S7].transform_0.data_1[i32(0)][i32(1)], instances_0[_S7].transform_0.data_1[i32(1)][i32(1)], instances_0[_S7].transform_0.data_1[i32(2)][i32(1)], instances_0[_S7].transform_0.data_1[i32(3)][i32(1)], instances_0[_S7].transform_0.data_1[i32(0)][i32(2)], instances_0[_S7].transform_0.data_1[i32(1)][i32(2)], instances_0[_S7].transform_0.data_1[i32(2)][i32(2)], instances_0[_S7].transform_0.data_1[i32(3)][i32(2)], instances_0[_S7].transform_0.data_1[i32(0)][i32(3)], instances_0[_S7].transform_0.data_1[i32(1)][i32(3)], instances_0[_S7].transform_0.data_1[i32(2)][i32(3)], instances_0[_S7].transform_0.data_1[i32(3)][i32(3)]);
    var _S13 : mat3x3<f32> = mat3x3<f32>(_S12[i32(0)].xyz, _S12[i32(1)].xyz, _S12[i32(2)].xyz);
    (*_S9) = (((vec4<f32>(_S11 * (bounds_max_0 + bounds_min_0), 1.0f)) * (_S12))).xyz;
    (*_S10) = (((local_extent_0) * (abs_0(_S13))));
    return;
}

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var inside_0 : bool;
    var index_0 : u32 = thread_0.x;
    if(index_0 >= (cull_0.instance_count_0))
    {
        return;
    }
    if(!admits_0(index_0))
    {
        return;
    }
    var _S14 : GpuMesh_std430_0 = meshes_0[instances_0[index_0].mesh_0];
    if((_S14.index_count_0) == u32(0))
    {
        return;
    }
    var face_0 : u32;
    var entry_0 : u32;
    if(has_box_0(index_0))
    {
        var center_2 : vec3<f32>;
        var extent_2 : vec3<f32>;
        world_box_0(index_0, &(_S14), &(center_2), &(extent_2));
        if(!in_frustum_0(center_2, extent_2))
        {
            return;
        }
        if((((cull_0.features_0) & (u32(1)))) != u32(0))
        {
            face_0 = u32(0);
            entry_0 = index_0;
            for(;;)
            {
                if(face_0 < u32(6))
                {
                }
                else
                {
                    break;
                }
                var plane_1 : u32 = u32(0);
                for(;;)
                {
                    if(plane_1 < u32(4))
                    {
                    }
                    else
                    {
                        inside_0 = true;
                        break;
                    }
                    if(!reaches_0(cull_0.face_planes_0[face_0 * u32(4) + plane_1], center_2, extent_2))
                    {
                        inside_0 = false;
                        break;
                    }
                    plane_1 = plane_1 + u32(1);
                }
                if(inside_0)
                {
                    entry_0 = (entry_0 | (((u32(1) << ((u32(24) + face_0))))));
                }
                face_0 = face_0 + u32(1);
            }
        }
        else
        {
            entry_0 = index_0;
        }
    }
    else
    {
        if((((cull_0.features_0) & (u32(1)))) != u32(0))
        {
            face_0 = (index_0 | (u32(1056964608)));
        }
        else
        {
            face_0 = index_0;
        }
        entry_0 = face_0;
    }
    var slot_0 : u32 = atomicAdd(&(visible_count_0[u32(0)]), u32(1));
    if(slot_0 < (cull_0.capacity_0))
    {
        visible_0[slot_0] = entry_0;
    }
    return;
}

struct ScreenBounds_0
{
     valid_0 : bool,
     min_x_1 : f32,
     min_y_1 : f32,
     max_x_1 : f32,
     max_y_1 : f32,
     nearest_0 : f32,
};

fn project_box_0( view_proj_1 : mat4x4<f32>,  center_3 : vec3<f32>,  extent_3 : vec3<f32>,  width_0 : f32,  height_0 : f32) -> ScreenBounds_0
{
    var bounds_0 : ScreenBounds_0;
    bounds_0.valid_0 = true;
    bounds_0.min_x_1 = 1.6777216e+07f;
    bounds_0.min_y_1 = 1.6777216e+07f;
    bounds_0.max_x_1 = -1.6777216e+07f;
    bounds_0.max_y_1 = -1.6777216e+07f;
    bounds_0.nearest_0 = 0.0f;
    var corner_0 : u32 = u32(0);
    for(;;)
    {
        if(corner_0 < u32(8))
        {
        }
        else
        {
            break;
        }
        var _S15 : f32;
        if(((corner_0 & (u32(1)))) != u32(0))
        {
            _S15 = 1.0f;
        }
        else
        {
            _S15 = -1.0f;
        }
        var _S16 : f32;
        if(((corner_0 & (u32(2)))) != u32(0))
        {
            _S16 = 1.0f;
        }
        else
        {
            _S16 = -1.0f;
        }
        var _S17 : f32;
        if(((corner_0 & (u32(4)))) != u32(0))
        {
            _S17 = 1.0f;
        }
        else
        {
            _S17 = -1.0f;
        }
        var clip_0 : vec4<f32> = (((vec4<f32>(center_3 + vec3<f32>(_S15, _S16, _S17) * extent_3, 1.0f)) * (view_proj_1)));
        var _S18 : f32 = clip_0.w;
        if(!(_S18 > 0.0f))
        {
            bounds_0.valid_0 = false;
            return bounds_0;
        }
        var depth_0 : f32 = clip_0.z / _S18;
        var x_1 : f32 = (clip_0.x / _S18 * 0.5f + 0.5f) * width_0;
        var y_0 : f32 = (0.5f - clip_0.y / _S18 * 0.5f) * height_0;
        var _S19 : bool;
        if(!(depth_0 <= 1.0f))
        {
            _S19 = true;
        }
        else
        {
            _S19 = !((abs(x_1)) < 1.6777216e+07f);
        }
        var _S20 : bool;
        if(_S19)
        {
            _S20 = true;
        }
        else
        {
            _S20 = !((abs(y_0)) < 1.6777216e+07f);
        }
        if(_S20)
        {
            bounds_0.valid_0 = false;
            return bounds_0;
        }
        bounds_0.min_x_1 = min(bounds_0.min_x_1, x_1);
        bounds_0.min_y_1 = min(bounds_0.min_y_1, y_0);
        bounds_0.max_x_1 = max(bounds_0.max_x_1, x_1);
        bounds_0.max_y_1 = max(bounds_0.max_y_1, y_0);
        bounds_0.nearest_0 = max(bounds_0.nearest_0, depth_0);
        corner_0 = corner_0 + u32(1);
    }
    return bounds_0;
}

fn pyramid_load_0( level_0 : u32,  texel_0 : vec2<i32>) -> f32
{
    var at_0 : vec3<i32> = vec3<i32>(texel_0, i32(0));
    switch(level_0)
    {
    case u32(1):
        {
            return (textureLoad((pyramid_1_0), ((at_0)).xy, ((at_0)).z));
        }
    case u32(2):
        {
            return (textureLoad((pyramid_2_0), ((at_0)).xy, ((at_0)).z));
        }
    case u32(3):
        {
            return (textureLoad((pyramid_3_0), ((at_0)).xy, ((at_0)).z));
        }
    case u32(4):
        {
            return (textureLoad((pyramid_4_0), ((at_0)).xy, ((at_0)).z));
        }
    case u32(5):
        {
            return (textureLoad((pyramid_5_0), ((at_0)).xy, ((at_0)).z));
        }
    case u32(6):
        {
            return (textureLoad((pyramid_6_0), ((at_0)).xy, ((at_0)).z));
        }
    case u32(7):
        {
            return (textureLoad((pyramid_7_0), ((at_0)).xy, ((at_0)).z));
        }
    default :
        {
            return (textureLoad((pyramid_8_0), ((at_0)).xy, ((at_0)).z));
        }
    }
}

fn occluded_0( view_proj_2 : mat4x4<f32>,  center_4 : vec3<f32>,  extent_4 : vec3<f32>) -> bool
{
    if((cull_0.pyramid_levels_0) == u32(0))
    {
        return false;
    }
    var bounds_1 : ScreenBounds_0 = project_box_0(view_proj_2, center_4, extent_4, f32(cull_0.target_width_0), f32(cull_0.target_height_0));
    if(!bounds_1.valid_0)
    {
        return false;
    }
    var width_1 : i32 = i32(cull_0.target_width_0);
    var height_1 : i32 = i32(cull_0.target_height_0);
    var x0_0 : i32 = i32(floor(bounds_1.min_x_1)) - i32(1);
    var y0_0 : i32 = i32(floor(bounds_1.min_y_1)) - i32(1);
    var x1_0 : i32 = i32(floor(bounds_1.max_x_1)) + i32(1);
    var y1_0 : i32 = i32(floor(bounds_1.max_y_1)) + i32(1);
    var _S21 : bool;
    if(x1_0 < i32(0))
    {
        _S21 = true;
    }
    else
    {
        _S21 = y1_0 < i32(0);
    }
    if(_S21)
    {
        _S21 = true;
    }
    else
    {
        _S21 = x0_0 >= width_1;
    }
    if(_S21)
    {
        _S21 = true;
    }
    else
    {
        _S21 = y0_0 >= height_1;
    }
    if(_S21)
    {
        return false;
    }
    var left_0 : u32 = u32(max(x0_0, i32(0)));
    var top_0 : u32 = u32(max(y0_0, i32(0)));
    var right_0 : u32 = u32(min(x1_0, width_1 - i32(1)));
    var bottom_0 : u32 = u32(min(y1_0, height_1 - i32(1)));
    var level_1 : u32 = u32(1);
    for(;;)
    {
        if(level_1 < (cull_0.pyramid_levels_0))
        {
            if((((right_0 >> (level_1))) - ((left_0 >> (level_1)))) > u32(1))
            {
                _S21 = true;
            }
            else
            {
                _S21 = (((bottom_0 >> (level_1))) - ((top_0 >> (level_1)))) > u32(1);
            }
        }
        else
        {
            _S21 = false;
        }
        if(_S21)
        {
        }
        else
        {
            break;
        }
        level_1 = level_1 + u32(1);
    }
    var _S22 : u32 = max(((cull_0.target_width_0) >> (level_1)), u32(1)) - u32(1);
    var _S23 : u32 = min((left_0 >> (level_1)), _S22);
    var _S24 : u32 = max(((cull_0.target_height_0) >> (level_1)), u32(1)) - u32(1);
    var _S25 : u32 = min((top_0 >> (level_1)), _S24);
    var _S26 : u32 = min((right_0 >> (level_1)), _S22);
    var _S27 : u32 = min((bottom_0 >> (level_1)), _S24);
    var farthest_0 : f32 = 1.0f;
    var y_1 : u32 = _S25;
    for(;;)
    {
        if(y_1 <= _S27)
        {
        }
        else
        {
            break;
        }
        var x_2 : u32 = _S23;
        for(;;)
        {
            if(x_2 <= _S26)
            {
            }
            else
            {
                break;
            }
            var _S28 : f32 = min(farthest_0, pyramid_load_0(level_1, vec2<i32>(i32(x_2), i32(y_1))));
            var x_3 : u32 = x_2 + u32(1);
            farthest_0 = _S28;
            x_2 = x_3;
        }
        y_1 = y_1 + u32(1);
    }
    return (bounds_1.nearest_0 * 1.000244140625f) < farthest_0;
}

@compute
@workgroup_size(64, 1, 1)
fn occlusionMain(@builtin(global_invocation_id) thread_1 : vec3<u32>)
{
    var index_1 : u32 = thread_1.x;
    if(index_1 >= (cull_0.instance_count_0))
    {
        return;
    }
    if(!admits_0(index_1))
    {
        return;
    }
    var _S29 : GpuMesh_std430_0 = meshes_0[instances_0[index_1].mesh_0];
    if((_S29.index_count_0) == u32(0))
    {
        return;
    }
    var entry_1 : u32;
    if(has_box_0(index_1))
    {
        var center_5 : vec3<f32>;
        var extent_5 : vec3<f32>;
        world_box_0(index_1, &(_S29), &(center_5), &(extent_5));
        if(!in_frustum_0(center_5, extent_5))
        {
            return;
        }
        var _S30 : bool;
        if((((cull_0.features_0) & (u32(4)))) != u32(0))
        {
            var bounds_2 : ScreenBounds_0 = project_box_0(mat4x4<f32>(cull_0.view_proj_0.data_0[i32(0)][i32(0)], cull_0.view_proj_0.data_0[i32(1)][i32(0)], cull_0.view_proj_0.data_0[i32(2)][i32(0)], cull_0.view_proj_0.data_0[i32(3)][i32(0)], cull_0.view_proj_0.data_0[i32(0)][i32(1)], cull_0.view_proj_0.data_0[i32(1)][i32(1)], cull_0.view_proj_0.data_0[i32(2)][i32(1)], cull_0.view_proj_0.data_0[i32(3)][i32(1)], cull_0.view_proj_0.data_0[i32(0)][i32(2)], cull_0.view_proj_0.data_0[i32(1)][i32(2)], cull_0.view_proj_0.data_0[i32(2)][i32(2)], cull_0.view_proj_0.data_0[i32(3)][i32(2)], cull_0.view_proj_0.data_0[i32(0)][i32(3)], cull_0.view_proj_0.data_0[i32(1)][i32(3)], cull_0.view_proj_0.data_0[i32(2)][i32(3)], cull_0.view_proj_0.data_0[i32(3)][i32(3)]), center_5, extent_5, f32(cull_0.target_width_0), f32(cull_0.target_height_0));
            if(bounds_2.valid_0)
            {
                _S30 = (max(bounds_2.max_x_1 - bounds_2.min_x_1, bounds_2.max_y_1 - bounds_2.min_y_1)) < (cull_0.small_feature_pixels_0);
            }
            else
            {
                _S30 = false;
            }
            if(_S30)
            {
                var _S31 : u32 = atomicAdd(&(visible_count_0[u32(7)]), u32(1));
                return;
            }
        }
        if((((cull_0.features_0) & (u32(2)))) != u32(0))
        {
            _S30 = occluded_0(mat4x4<f32>(cull_0.previous_view_proj_0.data_0[i32(0)][i32(0)], cull_0.previous_view_proj_0.data_0[i32(1)][i32(0)], cull_0.previous_view_proj_0.data_0[i32(2)][i32(0)], cull_0.previous_view_proj_0.data_0[i32(3)][i32(0)], cull_0.previous_view_proj_0.data_0[i32(0)][i32(1)], cull_0.previous_view_proj_0.data_0[i32(1)][i32(1)], cull_0.previous_view_proj_0.data_0[i32(2)][i32(1)], cull_0.previous_view_proj_0.data_0[i32(3)][i32(1)], cull_0.previous_view_proj_0.data_0[i32(0)][i32(2)], cull_0.previous_view_proj_0.data_0[i32(1)][i32(2)], cull_0.previous_view_proj_0.data_0[i32(2)][i32(2)], cull_0.previous_view_proj_0.data_0[i32(3)][i32(2)], cull_0.previous_view_proj_0.data_0[i32(0)][i32(3)], cull_0.previous_view_proj_0.data_0[i32(1)][i32(3)], cull_0.previous_view_proj_0.data_0[i32(2)][i32(3)], cull_0.previous_view_proj_0.data_0[i32(3)][i32(3)]), center_5, extent_5);
        }
        else
        {
            _S30 = false;
        }
        if(_S30)
        {
            var entry_2 : u32 = (index_1 | (u32(2147483648)));
            var _S32 : u32 = atomicAdd(&(visible_count_0[u32(5)]), u32(1));
            entry_1 = entry_2;
        }
        else
        {
            entry_1 = index_1;
        }
    }
    else
    {
        entry_1 = index_1;
    }
    var slot_1 : u32 = atomicAdd(&(visible_count_0[u32(0)]), u32(1));
    if(slot_1 < (cull_0.capacity_0))
    {
        visible_0[slot_1] = entry_1;
    }
    return;
}

@compute
@workgroup_size(64, 1, 1)
fn lateMain(@builtin(global_invocation_id) thread_2 : vec3<u32>)
{
    var index_2 : u32 = thread_2.x;
    var _S33 : u32 = atomicLoad(&(visible_count_0[u32(0)]));
    if(index_2 >= (min(_S33, cull_0.capacity_0)))
    {
        return;
    }
    var entry_3 : u32 = visible_0[index_2];
    if(((visible_0[index_2] & (u32(2147483648)))) == u32(0))
    {
        return;
    }
    var _S34 : u32 = (entry_3 & (u32(16777215)));
    var _S35 : GpuMesh_std430_0 = meshes_0[instances_0[_S34].mesh_0];
    var center_6 : vec3<f32>;
    var extent_6 : vec3<f32>;
    world_box_0(_S34, &(_S35), &(center_6), &(extent_6));
    if(occluded_0(mat4x4<f32>(cull_0.view_proj_0.data_0[i32(0)][i32(0)], cull_0.view_proj_0.data_0[i32(1)][i32(0)], cull_0.view_proj_0.data_0[i32(2)][i32(0)], cull_0.view_proj_0.data_0[i32(3)][i32(0)], cull_0.view_proj_0.data_0[i32(0)][i32(1)], cull_0.view_proj_0.data_0[i32(1)][i32(1)], cull_0.view_proj_0.data_0[i32(2)][i32(1)], cull_0.view_proj_0.data_0[i32(3)][i32(1)], cull_0.view_proj_0.data_0[i32(0)][i32(2)], cull_0.view_proj_0.data_0[i32(1)][i32(2)], cull_0.view_proj_0.data_0[i32(2)][i32(2)], cull_0.view_proj_0.data_0[i32(3)][i32(2)], cull_0.view_proj_0.data_0[i32(0)][i32(3)], cull_0.view_proj_0.data_0[i32(1)][i32(3)], cull_0.view_proj_0.data_0[i32(2)][i32(3)], cull_0.view_proj_0.data_0[i32(3)][i32(3)]), center_6, extent_6))
    {
        var _S36 : u32 = atomicAdd(&(visible_count_0[u32(6)]), u32(1));
        return;
    }
    visible_0[index_2] = (entry_3 | (u32(1073741824)));
    return;
}

