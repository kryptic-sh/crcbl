struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct LightClusterParams_std140_0
{
    @align(16) inverse_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) eye_0 : vec4<f32>,
    @align(16) depth_row_0 : vec4<f32>,
    @align(16) grid_x_0 : u32,
    @align(4) grid_y_0 : u32,
    @align(8) slices_0 : u32,
    @align(4) light_count_0 : u32,
    @align(16) viewport_x_0 : u32,
    @align(4) viewport_y_0 : u32,
    @align(8) perspective_0 : u32,
    @align(4) tile_pixels_0 : u32,
};

@binding(0) @group(0) var<uniform> params_0 : LightClusterParams_std140_0;
struct GpuLight_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
    @align(16) direction_0 : vec4<f32>,
    @align(16) kind_0 : u32,
    @align(4) cos_inner_0 : f32,
    @align(8) shadow_slot_0 : u32,
    @align(4) pad1_0 : u32,
};

@binding(1) @group(0) var<storage, read> lights_0 : array<GpuLight_std430_0>;

@binding(2) @group(0) var<storage, read_write> cluster_lights_0 : array<u32>;

@binding(3) @group(0) var<storage, read_write> cull_stats_0 : array<atomic<u32>>;

fn unproject_0( ndc_0 : vec2<f32>,  depth_0 : f32) -> vec3<f32>
{
    var world_0 : vec4<f32> = (((vec4<f32>(ndc_0, depth_0, 1.0f)) * (mat4x4<f32>(params_0.inverse_view_proj_0.data_0[i32(0)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(3)]))));
    return world_0.xyz / vec3<f32>(world_0.w);
}

fn view_depth_0( point_0 : vec3<f32>) -> f32
{
    return dot(params_0.depth_row_0, vec4<f32>(point_0, 1.0f));
}

fn slice_start_0( index_0 : u32) -> f32
{
    return 0.10000000149011612f * pow(10000.0f, f32(index_0) / 24.0f);
}

fn cone_touches_sphere_0( apex_0 : vec3<f32>,  axis_0 : vec3<f32>,  cos_outer_0 : f32,  range_0 : f32,  center_0 : vec3<f32>,  radius_0 : f32) -> bool
{
    var offset_0 : vec3<f32> = center_0 - apex_0;
    var along_0 : f32 = dot(offset_0, axis_0);
    var _S1 : bool;
    if(along_0 < (- radius_0))
    {
        _S1 = true;
    }
    else
    {
        _S1 = along_0 > (range_0 + radius_0);
    }
    if(_S1)
    {
        return false;
    }
    return (cos_outer_0 * sqrt(max(dot(offset_0, offset_0) - along_0 * along_0, 0.0f)) - along_0 * sqrt(saturate(1.0f - cos_outer_0 * cos_outer_0))) <= radius_0;
}

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var froxel_0 : u32 = thread_0.x;
    var tiles_0 : u32 = params_0.grid_x_0 * params_0.grid_y_0;
    if(froxel_0 >= (tiles_0 * params_0.slices_0))
    {
        return;
    }
    var tile_x_0 : u32 = froxel_0 % params_0.grid_x_0;
    var _S2 : u32 = froxel_0 / params_0.grid_x_0;
    var tile_y_0 : u32 = _S2 % params_0.grid_y_0;
    var slice_0 : u32 = froxel_0 / tiles_0;
    var _S3 : f32 = f32(params_0.viewport_x_0);
    var _S4 : f32 = f32(params_0.viewport_y_0);
    var pixel_min_0 : vec2<f32> = vec2<f32>(f32(tile_x_0), f32(tile_y_0)) * vec2<f32>(f32(params_0.tile_pixels_0));
    var pixel_max_0 : vec2<f32> = pixel_min_0 + vec2<f32>(f32(params_0.tile_pixels_0));
    var _S5 : f32 = pixel_min_0.x / _S3 * 2.0f - 1.0f;
    var _S6 : f32 = 1.0f - pixel_max_0.y / _S4 * 2.0f;
    var _S7 : f32 = pixel_max_0.x / _S3 * 2.0f - 1.0f;
    var _S8 : f32 = 1.0f - pixel_min_0.y / _S4 * 2.0f;
    var corner_near_0 : array<vec3<f32>, i32(4)>;
    corner_near_0[i32(0)] = unproject_0(vec2<f32>(_S5, _S6), 1.0f);
    corner_near_0[i32(1)] = unproject_0(vec2<f32>(_S7, _S6), 1.0f);
    corner_near_0[i32(2)] = unproject_0(vec2<f32>(_S5, _S8), 1.0f);
    corner_near_0[i32(3)] = unproject_0(vec2<f32>(_S7, _S8), 1.0f);
    var perspective_1 : bool = (params_0.perspective_0) != u32(0);
    const _S9 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var ortho_lo_0 : vec3<f32>;
    var ortho_hi_0 : vec3<f32>;
    var index_1 : u32;
    var _S10 : bool;
    var eye_to_near_0 : f32;
    var _S11 : f32;
    if(perspective_1)
    {
        eye_to_near_0 = view_depth_0(corner_near_0[i32(0)]);
        ortho_lo_0 = _S9;
        ortho_hi_0 = _S9;
    }
    else
    {
        ortho_lo_0 = corner_near_0[i32(0)];
        ortho_hi_0 = corner_near_0[i32(0)];
        index_1 = u32(0);
        for(;;)
        {
            if(index_1 < u32(4))
            {
            }
            else
            {
                break;
            }
            if(index_1 == u32(1))
            {
                _S10 = true;
            }
            else
            {
                _S10 = index_1 == u32(3);
            }
            if(_S10)
            {
                eye_to_near_0 = _S7;
            }
            else
            {
                eye_to_near_0 = _S5;
            }
            if(index_1 < u32(2))
            {
                _S11 = _S6;
            }
            else
            {
                _S11 = _S8;
            }
            var far_point_0 : vec3<f32> = unproject_0(vec2<f32>(eye_to_near_0, _S11), 0.0f);
            var _S12 : vec3<f32> = min(ortho_lo_0, min(corner_near_0[index_1], far_point_0));
            var _S13 : vec3<f32> = max(ortho_hi_0, max(corner_near_0[index_1], far_point_0));
            var corner_0 : u32 = index_1 + u32(1);
            ortho_lo_0 = _S12;
            ortho_hi_0 = _S13;
            index_1 = corner_0;
        }
        eye_to_near_0 = 1.0f;
    }
    if(perspective_1)
    {
        _S11 = slice_start_0(slice_0);
    }
    else
    {
        _S11 = 0.0f;
    }
    if(!perspective_1)
    {
        _S10 = true;
    }
    else
    {
        _S10 = (slice_0 + u32(1)) >= (params_0.slices_0);
    }
    var _S14 : f32;
    if(_S10)
    {
        _S14 = 3.4028234663852886e+38f;
    }
    else
    {
        _S14 = slice_start_0(slice_0 + u32(1));
    }
    var base_0 : u32 = froxel_0 * u32(17);
    index_1 = u32(0);
    var kept_0 : u32 = u32(0);
    var dropped_0 : u32 = u32(0);
    for(;;)
    {
        if(index_1 < (params_0.light_count_0))
        {
        }
        else
        {
            break;
        }
        var light_0 : GpuLight_std430_0 = lights_0[index_1];
        var kept_1 : u32;
        var touches_0 : bool;
        if((light_0.kind_0) == u32(0))
        {
            touches_0 = true;
        }
        else
        {
            var center_1 : vec3<f32> = light_0.position_0.xyz;
            var radius_1 : f32 = light_0.position_0.w;
            var box_lo_0 : vec3<f32>;
            var box_hi_0 : vec3<f32>;
            if(perspective_1)
            {
                var depth_1 : f32 = view_depth_0(center_1);
                var light_lo_0 : f32 = depth_1 - radius_1;
                var light_hi_0 : f32 = depth_1 + radius_1;
                if(light_hi_0 < _S11)
                {
                    _S10 = true;
                }
                else
                {
                    _S10 = light_lo_0 > _S14;
                }
                if(_S10)
                {
                    index_1 = index_1 + u32(1);
                    continue;
                }
                var _S15 : f32 = max(max(_S11, light_lo_0), _S11);
                var _S16 : f32 = max(min(_S14, light_hi_0), _S15);
                var first_0 : vec3<f32> = params_0.eye_0.xyz + (corner_near_0[i32(0)] - params_0.eye_0.xyz) * vec3<f32>((_S15 / eye_to_near_0));
                box_lo_0 = first_0;
                box_hi_0 = first_0;
                kept_1 = u32(0);
                for(;;)
                {
                    if(kept_1 < u32(4))
                    {
                    }
                    else
                    {
                        break;
                    }
                    var ray_0 : vec3<f32> = corner_near_0[kept_1] - params_0.eye_0.xyz;
                    var at_lo_0 : vec3<f32> = params_0.eye_0.xyz + ray_0 * vec3<f32>((_S15 / eye_to_near_0));
                    var at_hi_0 : vec3<f32> = params_0.eye_0.xyz + ray_0 * vec3<f32>((_S16 / eye_to_near_0));
                    var _S17 : vec3<f32> = min(box_lo_0, min(at_lo_0, at_hi_0));
                    var _S18 : vec3<f32> = max(box_hi_0, max(at_lo_0, at_hi_0));
                    var corner_1 : u32 = kept_1 + u32(1);
                    box_lo_0 = _S17;
                    box_hi_0 = _S18;
                    kept_1 = corner_1;
                }
            }
            else
            {
                box_lo_0 = ortho_lo_0;
                box_hi_0 = ortho_hi_0;
            }
            var offset_1 : vec3<f32> = center_1 - clamp(center_1, box_lo_0, box_hi_0);
            var touches_1 : bool = (dot(offset_1, offset_1)) <= (radius_1 * radius_1);
            if(touches_1)
            {
                _S10 = (light_0.kind_0) == u32(2);
            }
            else
            {
                _S10 = false;
            }
            if(_S10)
            {
                touches_0 = cone_touches_sphere_0(center_1, light_0.direction_0.xyz, light_0.direction_0.w, radius_1, (box_lo_0 + box_hi_0) * vec3<f32>(0.5f), length(box_hi_0 - box_lo_0) * 0.5f);
            }
            else
            {
                touches_0 = touches_1;
            }
        }
        if(!touches_0)
        {
            index_1 = index_1 + u32(1);
            continue;
        }
        var dropped_1 : u32;
        if(kept_0 < u32(16))
        {
            cluster_lights_0[base_0 + u32(1) + kept_0] = index_1;
            kept_1 = kept_0 + u32(1);
            dropped_1 = dropped_0;
        }
        else
        {
            var dropped_2 : u32 = dropped_0 + u32(1);
            kept_1 = kept_0;
            dropped_1 = dropped_2;
        }
        kept_0 = kept_1;
        dropped_0 = dropped_1;
        index_1 = index_1 + u32(1);
    }
    cluster_lights_0[base_0] = kept_0;
    if(dropped_0 > u32(0))
    {
        var _S19 : u32 = atomicAdd(&(cull_stats_0[u32(2)]), dropped_0);
    }
    return;
}

