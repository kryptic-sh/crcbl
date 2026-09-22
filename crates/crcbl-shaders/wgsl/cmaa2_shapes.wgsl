struct Cmaa2Params_std140_0
{
    @align(16) viewport_x_0 : u32,
    @align(4) viewport_y_0 : u32,
};

@binding(0) @group(0) var<uniform> params_0 : Cmaa2Params_std140_0;
@binding(2) @group(0) var<storage, read_write> edges_0 : array<u32>;

@binding(1) @group(0) var source_0 : texture_2d<f32>;

@binding(3) @group(0) var<storage, read_write> accum_0 : array<atomic<u32>>;

fn edge_at_0( x_0 : u32,  y_0 : u32) -> u32
{
    var _S1 : bool;
    if(x_0 >= (params_0.viewport_x_0))
    {
        _S1 = true;
    }
    else
    {
        _S1 = y_0 >= (params_0.viewport_y_0);
    }
    if(_S1)
    {
        return u32(0);
    }
    return edges_0[y_0 * params_0.viewport_x_0 + x_0];
}

fn horizontal_turn_0( column_0 : u32,  row_0 : u32) -> i32
{
    var below_0 : bool = (((edge_at_0(column_0, row_0)) & (u32(1)))) != u32(0);
    var above_0 : bool;
    if(row_0 > u32(0))
    {
        above_0 = (((edge_at_0(column_0, row_0 - u32(1))) & (u32(1)))) != u32(0);
    }
    else
    {
        above_0 = false;
    }
    var _S2 : bool;
    if(above_0)
    {
        _S2 = !below_0;
    }
    else
    {
        _S2 = false;
    }
    if(_S2)
    {
        return i32(1);
    }
    if(below_0)
    {
        above_0 = !above_0;
    }
    else
    {
        above_0 = false;
    }
    if(above_0)
    {
        return i32(-1);
    }
    return i32(0);
}

fn boundary_height_0( t_0 : f32,  start_0 : i32,  end_0 : i32,  u_shape_0 : bool) -> f32
{
    if(u_shape_0)
    {
        return -0.5f * f32(start_0) * (2.0f * abs(t_0 - 0.5f)) * 0.5f;
    }
    return -0.5f * f32(start_0) * (1.0f - t_0) - 0.5f * f32(end_0) * t_0;
}

fn accumulate_0( target_0 : u32,  from_0 : u32,  from_texel_0 : vec2<i32>,  share_0 : f32)
{
    if(share_0 < 0.001953125f)
    {
        return;
    }
    var pixels_0 : u32 = params_0.viewport_x_0 * params_0.viewport_y_0;
    var _S3 : bool;
    if(target_0 >= pixels_0)
    {
        _S3 = true;
    }
    else
    {
        _S3 = from_0 >= pixels_0;
    }
    if(_S3)
    {
        return;
    }
    var _S4 : vec3<i32> = vec3<i32>(from_texel_0, i32(0));
    var color_0 : vec3<f32> = saturate((textureLoad((source_0), ((_S4)).xy, ((_S4)).z)).xyz);
    var weight_0 : u32 = u32(min(share_0, 0.5f) * 1.048576e+06f);
    var scaled_0 : f32 = f32(weight_0);
    var _S5 : u32 = target_0 * u32(4);
    var _S6 : u32 = atomicAdd(&(accum_0[_S5]), u32(color_0.x * scaled_0));
    var _S7 : u32 = atomicAdd(&(accum_0[_S5 + u32(1)]), u32(color_0.y * scaled_0));
    var _S8 : u32 = atomicAdd(&(accum_0[_S5 + u32(2)]), u32(color_0.z * scaled_0));
    var _S9 : u32 = atomicAdd(&(accum_0[_S5 + u32(3)]), weight_0);
    return;
}

fn blend_line_0( first_0 : u32,  stride_0 : u32,  len_0 : u32,  offset_0 : u32,  start_1 : i32,  end_1 : i32,  first_texel_0 : vec2<i32>,  texel_step_0 : vec2<i32>,  across_0 : vec2<i32>)
{
    var _S10 : bool;
    if(start_1 == i32(0))
    {
        _S10 = true;
    }
    else
    {
        _S10 = end_1 == i32(0);
    }
    if(_S10)
    {
        return;
    }
    var _S11 : f32 = 1.0f / f32(len_0);
    var _S12 : bool = start_1 == end_1;
    var i_0 : u32 = u32(0);
    for(;;)
    {
        if(i_0 < len_0)
        {
        }
        else
        {
            break;
        }
        var near_0 : f32 = boundary_height_0(f32(i_0) * _S11, start_1, end_1, _S12);
        var _S13 : u32 = i_0 + u32(1);
        var far_0 : f32 = boundary_height_0(f32(_S13) * _S11, start_1, end_1, _S12);
        if(near_0 <= 0.0f)
        {
            _S10 = far_0 <= 0.0f;
        }
        else
        {
            _S10 = false;
        }
        var below_1 : f32;
        var above_1 : f32;
        if(_S10)
        {
            below_1 = -0.5f * (near_0 + far_0);
            above_1 = 0.0f;
        }
        else
        {
            var _S14 : bool;
            if(near_0 >= 0.0f)
            {
                _S14 = far_0 >= 0.0f;
            }
            else
            {
                _S14 = false;
            }
            if(_S14)
            {
                var _S15 : f32 = 0.5f * (near_0 + far_0);
                below_1 = 0.0f;
                above_1 = _S15;
            }
            else
            {
                var crossing_0 : f32 = near_0 / (near_0 - far_0);
                var _S16 : f32 = 0.5f * abs(min(near_0, far_0));
                if(near_0 < 0.0f)
                {
                    below_1 = crossing_0;
                }
                else
                {
                    below_1 = 1.0f - crossing_0;
                }
                var _S17 : f32 = _S16 * below_1;
                var _S18 : f32 = 0.5f * max(near_0, far_0);
                if(near_0 > 0.0f)
                {
                    above_1 = crossing_0;
                }
                else
                {
                    above_1 = 1.0f - crossing_0;
                }
                var _S19 : f32 = _S18 * above_1;
                below_1 = _S17;
                above_1 = _S19;
            }
        }
        var pixel_0 : u32 = first_0 + i_0 * stride_0;
        var texel_0 : vec2<i32> = first_texel_0 + vec2<i32>(i32(i_0)) * texel_step_0;
        var _S20 : u32 = pixel_0 - offset_0;
        accumulate_0(_S20, pixel_0, texel_0, below_1);
        accumulate_0(pixel_0, _S20, texel_0 - across_0, above_1);
        i_0 = _S13;
    }
    return;
}

fn vertical_turn_0( column_1 : u32,  row_1 : u32) -> i32
{
    var right_0 : bool = (((edge_at_0(column_1, row_1)) & (u32(2)))) != u32(0);
    var left_0 : bool;
    if(column_1 > u32(0))
    {
        left_0 = (((edge_at_0(column_1 - u32(1), row_1)) & (u32(2)))) != u32(0);
    }
    else
    {
        left_0 = false;
    }
    var _S21 : bool;
    if(left_0)
    {
        _S21 = !right_0;
    }
    else
    {
        _S21 = false;
    }
    if(_S21)
    {
        return i32(1);
    }
    if(right_0)
    {
        left_0 = !left_0;
    }
    else
    {
        left_0 = false;
    }
    if(left_0)
    {
        return i32(-1);
    }
    return i32(0);
}

@compute
@workgroup_size(64, 1, 1)
fn shapesMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var _S22 : bool;
    var _S23 : bool;
    var index_0 : u32 = thread_0.x;
    var width_0 : u32 = params_0.viewport_x_0;
    if(index_0 >= (params_0.viewport_x_0 * params_0.viewport_y_0))
    {
        return;
    }
    var own_0 : u32 = edges_0[index_0];
    if(edges_0[index_0] == u32(0))
    {
        return;
    }
    var x_1 : u32 = index_0 % width_0;
    var y_1 : u32 = index_0 / width_0;
    var _S24 : bool;
    if(((own_0 & (u32(2)))) != u32(0))
    {
        _S24 = (((edge_at_0(x_1 - u32(1), y_1)) & (u32(2)))) == u32(0);
    }
    else
    {
        _S24 = false;
    }
    var len_1 : u32;
    if(_S24)
    {
        len_1 = u32(1);
        for(;;)
        {
            var _S25 : bool = len_1 <= u32(64);
            _S22 = _S25;
            if(_S25)
            {
                _S24 = (((edge_at_0(x_1 + len_1, y_1)) & (u32(2)))) != u32(0);
            }
            else
            {
                _S24 = false;
            }
            if(_S24)
            {
            }
            else
            {
                break;
            }
            len_1 = len_1 + u32(1);
        }
        if(_S22)
        {
            blend_line_0(index_0, u32(1), len_1, width_0, horizontal_turn_0(x_1, y_1), horizontal_turn_0(x_1 + len_1, y_1), vec2<i32>(i32(x_1), i32(y_1)), vec2<i32>(i32(1), i32(0)), vec2<i32>(i32(0), i32(1)));
        }
    }
    if(((own_0 & (u32(1)))) != u32(0))
    {
        _S24 = (((edge_at_0(x_1, y_1 - u32(1))) & (u32(1)))) == u32(0);
    }
    else
    {
        _S24 = false;
    }
    if(_S24)
    {
        len_1 = u32(1);
        for(;;)
        {
            var _S26 : bool = len_1 <= u32(64);
            _S23 = _S26;
            if(_S26)
            {
                _S24 = (((edge_at_0(x_1, y_1 + len_1)) & (u32(1)))) != u32(0);
            }
            else
            {
                _S24 = false;
            }
            if(_S24)
            {
            }
            else
            {
                break;
            }
            len_1 = len_1 + u32(1);
        }
        if(_S23)
        {
            blend_line_0(index_0, width_0, len_1, u32(1), vertical_turn_0(x_1, y_1), vertical_turn_0(x_1, y_1 + len_1), vec2<i32>(i32(x_1), i32(y_1)), vec2<i32>(i32(0), i32(1)), vec2<i32>(i32(1), i32(0)));
        }
    }
    return;
}

