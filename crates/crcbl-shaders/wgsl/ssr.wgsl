@binding(1) @group(0) var scene_depth_0 : texture_depth_2d;

@binding(2) @group(0) var scene_color_0 : texture_2d<f32>;

@binding(3) @group(0) var reflectivity_0 : texture_2d<f32>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct SsrParams_std140_0
{
    @align(16) inv_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
};

@binding(0) @group(0) var<uniform> camera_0 : SsrParams_std140_0;
struct FullscreenOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) uv_0 : vec2<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> FullscreenOutput_0
{
    var output_0 : FullscreenOutput_0;
    var _S1 : vec2<f32> = vec2<f32>(f32((((index_0 << (u32(1)))) & (u32(2)))), f32((index_0 & (u32(2)))));
    output_0.uv_0 = _S1;
    output_0.position_0 = vec4<f32>(_S1 * vec2<f32>(2.0f, -2.0f) + vec2<f32>(-1.0f, 1.0f), 0.0f, 1.0f);
    return output_0;
}

fn depth_at_0( pixel_0 : vec2<i32>,  extent_0 : vec2<i32>) -> f32
{
    var _S2 : vec3<i32> = vec3<i32>(clamp(pixel_0, vec2<i32>(i32(0), i32(0)), extent_0 - vec2<i32>(i32(1), i32(1))), i32(0));
    return (textureLoad((scene_depth_0), ((_S2)).xy, ((_S2)).z));
}

fn view_position_0( pixel_1 : vec2<i32>,  depth_0 : f32,  extent_1 : vec2<f32>) -> vec3<f32>
{
    var view_0 : vec4<f32> = (((vec4<f32>(vec2<f32>((f32(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (f32(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
    return view_0.xyz / vec3<f32>(view_0.w);
}

fn normal_at_0( pixel_2 : vec2<i32>,  centre_0 : vec3<f32>,  extent_2 : vec2<i32>,  size_0 : vec2<f32>) -> vec3<f32>
{
    var _S3 : vec2<i32> = pixel_2 + vec2<i32>(i32(-1), i32(0));
    var left_0 : vec3<f32> = view_position_0(_S3, depth_at_0(_S3, extent_2), size_0);
    var _S4 : vec2<i32> = pixel_2 + vec2<i32>(i32(1), i32(0));
    var right_0 : vec3<f32> = view_position_0(_S4, depth_at_0(_S4, extent_2), size_0);
    var _S5 : vec2<i32> = pixel_2 + vec2<i32>(i32(0), i32(-1));
    var up_0 : vec3<f32> = view_position_0(_S5, depth_at_0(_S5, extent_2), size_0);
    var _S6 : vec2<i32> = pixel_2 + vec2<i32>(i32(0), i32(1));
    var down_0 : vec3<f32> = view_position_0(_S6, depth_at_0(_S6, extent_2), size_0);
    var _S7 : f32 = centre_0.z;
    var horizontal_0 : vec3<f32>;
    if((abs(right_0.z - _S7)) < (abs(_S7 - left_0.z)))
    {
        horizontal_0 = right_0 - centre_0;
    }
    else
    {
        horizontal_0 = centre_0 - left_0;
    }
    var vertical_0 : vec3<f32>;
    if((abs(down_0.z - _S7)) < (abs(_S7 - up_0.z)))
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

fn thickness_at_0( advance_0 : f32,  depth_1 : f32) -> f32
{
    return max(advance_0, abs(depth_1) * 0.01999999955296516f);
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_1 : vec2<f32>,
};

@fragment
fn fragmentMain( _S8 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var reflection_0 : vec3<f32>;
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((scene_depth_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var extent_3 : vec2<i32> = vec2<i32>(i32(width_0), i32(height_0));
    var _S9 : f32 = f32(width_0);
    var _S10 : f32 = f32(height_0);
    var size_3 : vec2<f32> = vec2<f32>(_S9, _S10);
    var _S11 : vec2<i32> = vec2<i32>(position_1.xy);
    var _S12 : vec3<i32> = vec3<i32>(_S11, i32(0));
    var lit_0 : vec4<f32> = (textureLoad((scene_color_0), ((_S12)).xy, ((_S12)).z));
    var surface_0 : vec4<f32> = (textureLoad((reflectivity_0), ((_S12)).xy, ((_S12)).z));
    var sharpness_0 : f32 = saturate(1.0f - surface_0.w / 0.5f);
    var depth_2 : f32 = depth_at_0(_S11, extent_3);
    var _S13 : bool;
    if(depth_2 <= 0.0f)
    {
        _S13 = true;
    }
    else
    {
        _S13 = sharpness_0 <= 0.0f;
    }
    if(_S13)
    {
        var _S14 : pixelOutput_0 = pixelOutput_0( lit_0 );
        return _S14;
    }
    var origin_0 : vec3<f32> = view_position_0(_S11, depth_2, size_3);
    var normal_0 : vec3<f32> = normal_at_0(_S11, origin_0, extent_3, size_3);
    var towards_0 : vec3<f32> = normalize(origin_0);
    var ray_0 : vec3<f32> = reflect(towards_0, normal_0);
    var _S15 : vec3<f32> = (vec3<f32>(0) - towards_0);
    var f0_0 : vec3<f32> = surface_0.xyz;
    var grazing_0 : f32 = 1.0f - saturate(dot(normal_0, _S15));
    var grazing2_0 : f32 = grazing_0 * grazing_0;
    var _S16 : vec3<f32> = f0_0 + (vec3<f32>(1.0f, 1.0f, 1.0f) - f0_0) * vec3<f32>((grazing2_0 * grazing2_0 * grazing_0));
    var facing_0 : f32 = saturate((1.0f - dot(ray_0, _S15)) / 0.05000000074505806f);
    if(facing_0 <= 0.0f)
    {
        var _S17 : pixelOutput_0 = pixelOutput_0( lit_0 );
        return _S17;
    }
    var _S18 : f32 = origin_0.z;
    var start_0 : vec3<f32> = origin_0 + normal_0 * vec3<f32>((abs(_S18) * 0.00499999988824129f));
    var clip_start_0 : vec4<f32> = (((vec4<f32>(start_0, 1.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var clip_ray_0 : vec4<f32> = (((vec4<f32>(ray_0, 0.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var _S19 : f32 = clip_start_0.w;
    if(_S19 <= 0.0f)
    {
        var _S20 : pixelOutput_0 = pixelOutput_0( lit_0 );
        return _S20;
    }
    var _S21 : vec2<f32> = clip_start_0.xy;
    var _S22 : vec2<f32> = vec2<f32>(_S19);
    var at_start_0 : vec2<f32> = pixel_of_0(_S21 / _S22, size_3);
    var _S23 : vec2<f32> = clip_ray_0.xy;
    var _S24 : f32 = clip_ray_0.w;
    var _S25 : vec2<f32> = vec2<f32>(_S24);
    var ndc_rate_0 : vec2<f32> = (_S23 * _S22 - _S21 * _S25) / vec2<f32>((_S19 * _S19));
    var screen_rate_0 : vec2<f32> = vec2<f32>(ndc_rate_0.x * 0.5f * _S9, - ndc_rate_0.y * 0.5f * _S10);
    var rate_0 : f32 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {
        var _S26 : pixelOutput_0 = pixelOutput_0( lit_0 );
        return _S26;
    }
    var forward_0 : vec2<f32> = screen_rate_0 / vec2<f32>(rate_0);
    var stride_0 : f32 = 0.75f * min(_S9, _S10) / 96.0f;
    var travel_0 : f32 = 96.0f * stride_0;
    var _S27 : f32 = forward_0.x;
    var travel_1 : f32;
    if(_S27 > 0.0f)
    {
        travel_1 = min(travel_0, (_S9 - 1.0f - at_start_0.x) / _S27);
    }
    else
    {
        if(_S27 < 0.0f)
        {
            travel_1 = min(travel_0, - at_start_0.x / _S27);
        }
        else
        {
            travel_1 = travel_0;
        }
    }
    var _S28 : f32 = forward_0.y;
    if(_S28 > 0.0f)
    {
        travel_1 = min(travel_1, (_S10 - 1.0f - at_start_0.y) / _S28);
    }
    else
    {
        if(_S28 < 0.0f)
        {
            travel_1 = min(travel_1, - at_start_0.y / _S28);
        }
    }
    if(_S24 > 0.0f)
    {
        travel_1 = min(travel_1, max(dot(pixel_of_0(_S23 / _S25, size_3) - at_start_0, forward_0), 0.0f));
    }
    else
    {
        if(_S24 < 0.0f)
        {
            var on_near_0 : vec4<f32> = (((vec4<f32>(0.0f, 0.0f, 1.0f, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
            var clip_near_0 : vec4<f32> = clip_start_0 + clip_ray_0 * vec4<f32>(((- on_near_0.z / on_near_0.w - _S19) / _S24));
            travel_1 = min(travel_1, max(dot(pixel_of_0(clip_near_0.xy / vec2<f32>(clip_near_0.w), size_3) - at_start_0, forward_0), 0.0f));
        }
    }
    var steps_0 : u32 = u32(max(travel_1, 0.0f) / stride_0);
    if(steps_0 == u32(0))
    {
        var _S29 : pixelOutput_0 = pixelOutput_0( lit_0 );
        return _S29;
    }
    var _S30 : f32 = f32(steps_0);
    var travel_2 : f32 = _S30 * stride_0;
    var ndc_end_0 : vec2<f32> = ndc_of_0(at_start_0 + forward_0 * vec2<f32>(travel_2), size_3);
    var when_end_0 : f32;
    if((abs(_S27)) >= (abs(_S28)))
    {
        var _S31 : f32 = ndc_end_0.x;
        when_end_0 = (_S31 * _S19 - clip_start_0.x) / (clip_ray_0.x - _S31 * _S24);
    }
    else
    {
        var _S32 : f32 = ndc_end_0.y;
        when_end_0 = (_S32 * _S19 - clip_start_0.y) / (clip_ray_0.y - _S32 * _S24);
    }
    if(!(when_end_0 > 0.0f))
    {
        var _S33 : pixelOutput_0 = pixelOutput_0( lit_0 );
        return _S33;
    }
    var inverse_w_start_0 : f32 = 1.0f / _S19;
    var inverse_w_end_0 : f32 = 1.0f / (_S19 + when_end_0 * _S24);
    var _S34 : f32 = start_0.z;
    var _S35 : f32 = _S34 * inverse_w_start_0;
    var _S36 : f32 = (_S34 + when_end_0 * ray_0.z) * inverse_w_end_0;
    const _S37 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var previous_gap_0 : f32 = _S34 - _S18;
    var previous_z_0 : f32 = _S34;
    var previous_at_0 : vec2<f32> = at_start_0;
    var step_0 : u32 = u32(1);
    for(;;)
    {
        if(step_0 <= steps_0)
        {
        }
        else
        {
            reflection_0 = _S37;
            break;
        }
        var _S38 : f32 = f32(step_0);
        var along_0 : f32 = _S38 / _S30;
        var at_1 : vec2<f32> = at_start_0 + forward_0 * vec2<f32>((travel_2 * along_0));
        var _S39 : vec2<i32> = vec2<i32>(at_1);
        var ray_z_0 : f32 = mix(_S35, _S36, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);
        var tapped_0 : f32 = depth_at_0(_S39, extent_3);
        var gap_0 : f32;
        if(tapped_0 > 0.0f)
        {
            gap_0 = ray_z_0 - view_position_0(_S39, tapped_0, size_3).z;
        }
        else
        {
            gap_0 = 1.0f;
        }
        if(previous_gap_0 > 0.0f)
        {
            _S13 = gap_0 < 0.0f;
        }
        else
        {
            _S13 = false;
        }
        if(_S13)
        {
            var behind_0 : f32 = - gap_0;
            var thickness_0 : f32 = thickness_at_0(abs(ray_z_0 - previous_z_0), ray_z_0);
            if(behind_0 <= thickness_0)
            {
                var hit_at_0 : vec2<f32> = mix(previous_at_0, at_1, vec2<f32>((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))));
                var hit_ndc_0 : vec2<f32> = ndc_of_0(hit_at_0, size_3);
                var _S40 : vec3<i32> = vec3<i32>(clamp(vec2<i32>(hit_at_0), vec2<i32>(i32(0), i32(0)), extent_3 - vec2<i32>(i32(1), i32(1))), i32(0));
                reflection_0 = (textureLoad((scene_color_0), ((_S40)).xy, ((_S40)).z)).xyz * _S16 * vec3<f32>((sharpness_0 * facing_0 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S38 / 96.0f) / 0.25f) * saturate(1.0f - behind_0 / thickness_0)));
                break;
            }
        }
        var step_1 : u32 = step_0 + u32(1);
        previous_gap_0 = gap_0;
        previous_z_0 = ray_z_0;
        previous_at_0 = at_1;
        step_0 = step_1;
    }
    var _S41 : pixelOutput_0 = pixelOutput_0( vec4<f32>(lit_0.xyz + reflection_0, lit_0.w) );
    return _S41;
}

