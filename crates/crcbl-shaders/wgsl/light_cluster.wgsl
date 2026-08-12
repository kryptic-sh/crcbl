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
    @align(8) pad0_0 : u32,
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
    var _S1 : u32 = froxel_0 / params_0.grid_x_0;
    var tile_y_0 : u32 = _S1 % params_0.grid_y_0;
    var slice_0 : u32 = froxel_0 / tiles_0;
    var _S2 : f32 = f32(params_0.viewport_x_0);
    var _S3 : f32 = f32(params_0.viewport_y_0);
    var pixel_min_0 : vec2<f32> = vec2<f32>(f32(tile_x_0), f32(tile_y_0)) * vec2<f32>(f32(params_0.tile_pixels_0));
    var pixel_max_0 : vec2<f32> = pixel_min_0 + vec2<f32>(f32(params_0.tile_pixels_0));
    var _S4 : f32 = pixel_min_0.x / _S2 * 2.0f - 1.0f;
    var _S5 : f32 = 1.0f - pixel_max_0.y / _S3 * 2.0f;
    var _S6 : f32 = pixel_max_0.x / _S2 * 2.0f - 1.0f;
    var _S7 : f32 = 1.0f - pixel_min_0.y / _S3 * 2.0f;
    var corner_near_0 : array<vec3<f32>, i32(4)>;
    corner_near_0[i32(0)] = unproject_0(vec2<f32>(_S4, _S5), 1.0f);
    corner_near_0[i32(1)] = unproject_0(vec2<f32>(_S6, _S5), 1.0f);
    corner_near_0[i32(2)] = unproject_0(vec2<f32>(_S4, _S7), 1.0f);
    corner_near_0[i32(3)] = unproject_0(vec2<f32>(_S6, _S7), 1.0f);
    var perspective_1 : bool = (params_0.perspective_0) != u32(0);
    const _S8 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var ortho_lo_0 : vec3<f32>;
    var ortho_hi_0 : vec3<f32>;
    var index_1 : u32;
    var touches_0 : bool;
    var eye_to_near_0 : f32;
    var _S9 : f32;
    if(perspective_1)
    {
        eye_to_near_0 = view_depth_0(corner_near_0[i32(0)]);
        ortho_lo_0 = _S8;
        ortho_hi_0 = _S8;
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
                touches_0 = true;
            }
            else
            {
                touches_0 = index_1 == u32(3);
            }
            if(touches_0)
            {
                eye_to_near_0 = _S6;
            }
            else
            {
                eye_to_near_0 = _S4;
            }
            if(index_1 < u32(2))
            {
                _S9 = _S5;
            }
            else
            {
                _S9 = _S7;
            }
            var far_point_0 : vec3<f32> = unproject_0(vec2<f32>(eye_to_near_0, _S9), 0.0f);
            var _S10 : vec3<f32> = min(ortho_lo_0, min(corner_near_0[index_1], far_point_0));
            var _S11 : vec3<f32> = max(ortho_hi_0, max(corner_near_0[index_1], far_point_0));
            var corner_0 : u32 = index_1 + u32(1);
            ortho_lo_0 = _S10;
            ortho_hi_0 = _S11;
            index_1 = corner_0;
        }
        eye_to_near_0 = 1.0f;
    }
    if(perspective_1)
    {
        _S9 = slice_start_0(slice_0);
    }
    else
    {
        _S9 = 0.0f;
    }
    if(!perspective_1)
    {
        touches_0 = true;
    }
    else
    {
        touches_0 = (slice_0 + u32(1)) >= (params_0.slices_0);
    }
    var _S12 : f32;
    if(touches_0)
    {
        _S12 = 3.4028234663852886e+38f;
    }
    else
    {
        _S12 = slice_start_0(slice_0 + u32(1));
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
        if((light_0.kind_0) == u32(0))
        {
            touches_0 = true;
        }
        else
        {
            var center_0 : vec3<f32> = light_0.position_0.xyz;
            var radius_0 : f32 = light_0.position_0.w;
            var box_lo_0 : vec3<f32>;
            var box_hi_0 : vec3<f32>;
            if(perspective_1)
            {
                var depth_1 : f32 = view_depth_0(center_0);
                var light_lo_0 : f32 = depth_1 - radius_0;
                var light_hi_0 : f32 = depth_1 + radius_0;
                if(light_hi_0 < _S9)
                {
                    touches_0 = true;
                }
                else
                {
                    touches_0 = light_lo_0 > _S12;
                }
                if(touches_0)
                {
                    index_1 = index_1 + u32(1);
                    continue;
                }
                var _S13 : f32 = max(max(_S9, light_lo_0), _S9);
                var _S14 : f32 = max(min(_S12, light_hi_0), _S13);
                var first_0 : vec3<f32> = params_0.eye_0.xyz + (corner_near_0[i32(0)] - params_0.eye_0.xyz) * vec3<f32>((_S13 / eye_to_near_0));
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
                    var at_lo_0 : vec3<f32> = params_0.eye_0.xyz + ray_0 * vec3<f32>((_S13 / eye_to_near_0));
                    var at_hi_0 : vec3<f32> = params_0.eye_0.xyz + ray_0 * vec3<f32>((_S14 / eye_to_near_0));
                    var _S15 : vec3<f32> = min(box_lo_0, min(at_lo_0, at_hi_0));
                    var _S16 : vec3<f32> = max(box_hi_0, max(at_lo_0, at_hi_0));
                    var corner_1 : u32 = kept_1 + u32(1);
                    box_lo_0 = _S15;
                    box_hi_0 = _S16;
                    kept_1 = corner_1;
                }
            }
            else
            {
                box_lo_0 = ortho_lo_0;
                box_hi_0 = ortho_hi_0;
            }
            var offset_0 : vec3<f32> = center_0 - clamp(center_0, box_lo_0, box_hi_0);
            touches_0 = (dot(offset_0, offset_0)) <= (radius_0 * radius_0);
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
        var _S17 : u32 = atomicAdd(&(cull_stats_0[u32(2)]), dropped_0);
    }
    return;
}

