@binding(1) @group(0) var scene_depth_0 : texture_depth_2d;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct ContactShadowParams_std140_0
{
    @align(16) inv_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) to_light_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> camera_0 : ContactShadowParams_std140_0;
fn isnan_0( x_0 : f32) -> bool
{
    var _S1 : u32 = (bitcast<u32>((x_0)));
    var _S2 : u32 = (_S1 & (u32(8388607)));
    var _S3 : bool;
    if(((((_S1 >> (u32(23)))) & (u32(255)))) == u32(255))
    {
        _S3 = _S2 != u32(0);
    }
    else
    {
        _S3 = false;
    }
    return _S3;
}

fn isinf_0( x_1 : f32) -> bool
{
    var _S4 : u32 = (bitcast<u32>((x_1)));
    var _S5 : u32 = (_S4 & (u32(8388607)));
    var _S6 : bool;
    if(((((_S4 >> (u32(23)))) & (u32(255)))) == u32(255))
    {
        _S6 = _S5 == u32(0);
    }
    else
    {
        _S6 = false;
    }
    return _S6;
}

fn isfinite_0( x_2 : f32) -> bool
{
    var _S7 : bool;
    if(isinf_0(x_2))
    {
        _S7 = true;
    }
    else
    {
        _S7 = isnan_0(x_2);
    }
    return !_S7;
}

struct FullscreenOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) uv_0 : vec2<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> FullscreenOutput_0
{
    var output_0 : FullscreenOutput_0;
    var _S8 : vec2<f32> = vec2<f32>(f32((((index_0 << (u32(1)))) & (u32(2)))), f32((index_0 & (u32(2)))));
    output_0.uv_0 = _S8;
    output_0.position_0 = vec4<f32>(_S8 * vec2<f32>(2.0f, -2.0f) + vec2<f32>(-1.0f, 1.0f), 0.0f, 1.0f);
    return output_0;
}

fn depth_at_0( pixel_0 : vec2<i32>,  extent_0 : vec2<i32>) -> f32
{
    var _S9 : vec3<i32> = vec3<i32>(clamp(pixel_0, vec2<i32>(i32(0), i32(0)), extent_0 - vec2<i32>(i32(1), i32(1))), i32(0));
    return (textureLoad((scene_depth_0), ((_S9)).xy, ((_S9)).z));
}

fn view_position_0( pixel_1 : vec2<i32>,  depth_0 : f32,  extent_1 : vec2<f32>) -> vec3<f32>
{
    var view_0 : vec4<f32> = (((vec4<f32>(vec2<f32>((f32(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (f32(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
    return view_0.xyz / vec3<f32>(view_0.w);
}

fn normal_at_0( pixel_2 : vec2<i32>,  centre_0 : vec3<f32>,  extent_2 : vec2<i32>,  size_0 : vec2<f32>) -> vec3<f32>
{
    var _S10 : vec2<i32> = pixel_2 + vec2<i32>(i32(-1), i32(0));
    var left_0 : vec3<f32> = view_position_0(_S10, depth_at_0(_S10, extent_2), size_0);
    var _S11 : vec2<i32> = pixel_2 + vec2<i32>(i32(1), i32(0));
    var right_0 : vec3<f32> = view_position_0(_S11, depth_at_0(_S11, extent_2), size_0);
    var _S12 : vec2<i32> = pixel_2 + vec2<i32>(i32(0), i32(-1));
    var up_0 : vec3<f32> = view_position_0(_S12, depth_at_0(_S12, extent_2), size_0);
    var _S13 : vec2<i32> = pixel_2 + vec2<i32>(i32(0), i32(1));
    var down_0 : vec3<f32> = view_position_0(_S13, depth_at_0(_S13, extent_2), size_0);
    var _S14 : f32 = centre_0.z;
    var horizontal_0 : vec3<f32>;
    if((abs(right_0.z - _S14)) < (abs(_S14 - left_0.z)))
    {
        horizontal_0 = right_0 - centre_0;
    }
    else
    {
        horizontal_0 = centre_0 - left_0;
    }
    var vertical_0 : vec3<f32>;
    if((abs(down_0.z - _S14)) < (abs(_S14 - up_0.z)))
    {
        vertical_0 = down_0 - centre_0;
    }
    else
    {
        vertical_0 = centre_0 - up_0;
    }
    return normalize(cross(vertical_0, horizontal_0));
}

fn pixel_of_0( ndc_0 : vec2<f32>,  size_1 : vec2<f32>) -> vec2<f32>
{
    return vec2<f32>((ndc_0.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_0.y * 0.5f) * size_1.y);
}

fn ndc_of_0( at_0 : vec2<f32>,  size_2 : vec2<f32>) -> vec2<f32>
{
    return vec2<f32>(at_0.x / size_2.x * 2.0f - 1.0f, 1.0f - at_0.y / size_2.y * 2.0f);
}

fn cell_exit_0( at_1 : vec2<f32>,  forward_0 : vec2<f32>,  size_3 : f32,  reach_0 : f32) -> f32
{
    var _S15 : f32 = forward_0.x;
    var _S16 : bool = _S15 > 0.0f;
    var along_x_0 : f32;
    if(_S16)
    {
        along_x_0 = (floor(at_1.x / size_3) + 1.0f) * size_3;
    }
    else
    {
        along_x_0 = floor(at_1.x / size_3) * size_3;
    }
    var _S17 : f32 = forward_0.y;
    var _S18 : bool = _S17 > 0.0f;
    var along_y_0 : f32;
    if(_S18)
    {
        along_y_0 = (floor(at_1.y / size_3) + 1.0f) * size_3;
    }
    else
    {
        along_y_0 = floor(at_1.y / size_3) * size_3;
    }
    var nudge_0 : f32 = size_3 * 0.00390625f;
    var _S19 : f32;
    if((abs(_S15)) < 9.99999997475242708e-07f)
    {
        along_x_0 = reach_0;
    }
    else
    {
        if(_S16)
        {
            _S19 = nudge_0;
        }
        else
        {
            _S19 = - nudge_0;
        }
        along_x_0 = (along_x_0 + _S19 - at_1.x) / _S15;
    }
    if((abs(_S17)) < 9.99999997475242708e-07f)
    {
        along_y_0 = reach_0;
    }
    else
    {
        if(_S18)
        {
            _S19 = nudge_0;
        }
        else
        {
            _S19 = - nudge_0;
        }
        along_y_0 = (along_y_0 + _S19 - at_1.y) / _S17;
    }
    return max(min(along_x_0, along_y_0), nudge_0);
}

fn view_z_of_0( depth_1 : f32) -> f32
{
    var view_1 : vec4<f32> = (((vec4<f32>(0.0f, 0.0f, depth_1, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
    return view_1.z / view_1.w;
}

fn thickness_at_0( advance_0 : f32,  depth_2 : f32) -> f32
{
    return max(advance_0, abs(depth_2) * 0.01999999955296516f);
}

struct pixelOutput_0
{
    @location(0) output_1 : f32,
};

struct pixelInput_0
{
    @location(0) uv_1 : vec2<f32>,
};

@fragment
fn fragmentMain( _S20 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((scene_depth_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var extent_3 : vec2<i32> = vec2<i32>(i32(width_0), i32(height_0));
    var _S21 : f32 = f32(width_0);
    var _S22 : f32 = f32(height_0);
    var size_4 : vec2<f32> = vec2<f32>(_S21, _S22);
    var _S23 : vec2<i32> = vec2<i32>(position_1.xy);
    var depth_3 : f32 = depth_at_0(_S23, extent_3);
    if(depth_3 <= 0.0f)
    {
        var _S24 : pixelOutput_0 = pixelOutput_0( 1.0f );
        return _S24;
    }
    var origin_0 : vec3<f32> = view_position_0(_S23, depth_3, size_4);
    var normal_0 : vec3<f32> = normal_at_0(_S23, origin_0, extent_3, size_4);
    var ray_0 : vec3<f32> = camera_0.to_light_0.xyz;
    var facing_0 : f32 = saturate(dot(normal_0, ray_0) / 0.10000000149011612f);
    if(facing_0 <= 0.0f)
    {
        var _S25 : pixelOutput_0 = pixelOutput_0( 1.0f );
        return _S25;
    }
    var _S26 : f32 = origin_0.z;
    var start_0 : vec3<f32> = origin_0 + normal_0 * vec3<f32>((abs(_S26) * 0.00499999988824129f));
    var clip_start_0 : vec4<f32> = (((vec4<f32>(start_0, 1.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var clip_ray_0 : vec4<f32> = (((vec4<f32>(ray_0, 0.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var _S27 : f32 = clip_start_0.w;
    if(_S27 <= 0.0f)
    {
        var _S28 : pixelOutput_0 = pixelOutput_0( 1.0f );
        return _S28;
    }
    var _S29 : vec2<f32> = clip_start_0.xy;
    var _S30 : vec2<f32> = vec2<f32>(_S27);
    var at_start_0 : vec2<f32> = pixel_of_0(_S29 / _S30, size_4);
    var _S31 : f32 = clip_ray_0.w;
    var ndc_rate_0 : vec2<f32> = (clip_ray_0.xy * _S30 - _S29 * vec2<f32>(_S31)) / vec2<f32>((_S27 * _S27));
    var screen_rate_0 : vec2<f32> = vec2<f32>(ndc_rate_0.x * 0.5f * _S21, - ndc_rate_0.y * 0.5f * _S22);
    var rate_0 : f32 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {
        var _S32 : pixelOutput_0 = pixelOutput_0( 1.0f );
        return _S32;
    }
    var forward_1 : vec2<f32> = screen_rate_0 / vec2<f32>(rate_0);
    var clip_end_0 : vec4<f32> = (((vec4<f32>(start_0 + ray_0 * vec3<f32>(0.25f), 1.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var _S33 : f32 = clip_end_0.w;
    var travel_0 : f32;
    if(_S33 > 0.0f)
    {
        travel_0 = min(15.0f, max(dot(pixel_of_0(clip_end_0.xy / vec2<f32>(_S33), size_4) - at_start_0, forward_1), 0.0f));
    }
    else
    {
        travel_0 = 15.0f;
    }
    var _S34 : f32 = forward_1.x;
    if(_S34 > 0.0f)
    {
        travel_0 = min(travel_0, (_S21 - 1.0f - at_start_0.x) / _S34);
    }
    else
    {
        if(_S34 < 0.0f)
        {
            travel_0 = min(travel_0, - at_start_0.x / _S34);
        }
    }
    var _S35 : f32 = forward_1.y;
    if(_S35 > 0.0f)
    {
        travel_0 = min(travel_0, (_S22 - 1.0f - at_start_0.y) / _S35);
    }
    else
    {
        if(_S35 < 0.0f)
        {
            travel_0 = min(travel_0, - at_start_0.y / _S35);
        }
    }
    if(_S31 < 0.0f)
    {
        var on_near_0 : vec4<f32> = (((vec4<f32>(0.0f, 0.0f, 1.0f, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
        var clip_near_0 : vec4<f32> = clip_start_0 + clip_ray_0 * vec4<f32>(((- on_near_0.z / on_near_0.w - _S27) / _S31));
        travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / vec2<f32>(clip_near_0.w), size_4) - at_start_0, forward_1), 0.0f));
    }
    var _S36 : f32 = max(travel_0, 0.0f);
    if(_S36 < 2.0f)
    {
        var _S37 : pixelOutput_0 = pixelOutput_0( 1.0f );
        return _S37;
    }
    var ndc_end_0 : vec2<f32> = ndc_of_0(at_start_0 + forward_1 * vec2<f32>(_S36), size_4);
    var when_end_0 : f32;
    if((abs(_S34)) >= (abs(_S35)))
    {
        var _S38 : f32 = ndc_end_0.x;
        when_end_0 = (_S38 * _S27 - clip_start_0.x) / (clip_ray_0.x - _S38 * _S31);
    }
    else
    {
        var _S39 : f32 = ndc_end_0.y;
        when_end_0 = (_S39 * _S27 - clip_start_0.y) / (clip_ray_0.y - _S39 * _S31);
    }
    var _S40 : bool;
    if(!(when_end_0 > 0.0f))
    {
        _S40 = true;
    }
    else
    {
        _S40 = !isfinite_0(when_end_0);
    }
    if(_S40)
    {
        var _S41 : pixelOutput_0 = pixelOutput_0( 1.0f );
        return _S41;
    }
    var inverse_w_start_0 : f32 = 1.0f / _S27;
    var inverse_w_end_0 : f32 = 1.0f / (_S27 + when_end_0 * _S31);
    var _S42 : f32 = start_0.z;
    var _S43 : f32 = _S42 * inverse_w_start_0;
    var _S44 : f32 = (_S42 + when_end_0 * ray_0.z) * inverse_w_end_0;
    var _S45 : f32 = _S42 - _S26;
    var at_travel_0 : f32 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S36), _S36);
    var previous_gap_0 : f32 = _S45;
    var entry_z_0 : f32 = _S42;
    var step_0 : u32 = u32(0);
    for(;;)
    {
        if(step_0 < u32(16))
        {
        }
        else
        {
            break;
        }
        var at_2 : vec2<f32> = at_start_0 + forward_1 * vec2<f32>(at_travel_0);
        var _S46 : f32 = min(at_travel_0 + cell_exit_0(at_2, forward_1, 1.0f, _S36), _S36);
        var exit_at_0 : vec2<f32> = at_start_0 + forward_1 * vec2<f32>(_S46);
        var along_0 : f32 = _S46 / _S36;
        var exit_z_0 : f32 = mix(_S43, _S44, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);
        var cell_depth_0 : f32 = depth_at_0(vec2<i32>(floor(at_2)), extent_3);
        var gap_0 : f32;
        if(cell_depth_0 <= 0.0f)
        {
            gap_0 = 1.0f;
        }
        else
        {
            gap_0 = exit_z_0 - view_z_of_0(cell_depth_0);
        }
        if(gap_0 <= 0.0f)
        {
            _S40 = previous_gap_0 > 0.0f;
        }
        else
        {
            _S40 = false;
        }
        if(_S40)
        {
            var behind_0 : f32 = - gap_0;
            var thickness_0 : f32 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {
                var hit_ndc_0 : vec2<f32> = ndc_of_0(mix(at_2, exit_at_0, vec2<f32>((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f)))), size_4);
                var _S47 : pixelOutput_0 = pixelOutput_0( saturate(1.0f - facing_0 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S46 / _S36) / 0.25f) * saturate(1.0f - behind_0 / thickness_0)) );
                return _S47;
            }
        }
        if(_S46 >= _S36)
        {
            break;
        }
        var step_1 : u32 = step_0 + u32(1);
        at_travel_0 = _S46;
        previous_gap_0 = gap_0;
        entry_z_0 = exit_z_0;
        step_0 = step_1;
    }
    var _S48 : pixelOutput_0 = pixelOutput_0( 1.0f );
    return _S48;
}

