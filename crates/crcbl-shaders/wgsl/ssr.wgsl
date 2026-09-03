@binding(1) @group(0) var scene_depth_0 : texture_depth_2d;

@binding(3) @group(0) var reflectivity_0 : texture_2d<f32>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct SsrParams_std140_0
{
    @align(16) inv_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) inv_view_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) probe_counts_0 : vec4<u32>,
    @align(16) probe_levels_0 : vec4<u32>,
    @align(16) probe_level_origin_0 : array<vec4<f32>, i32(4)>,
    @align(16) probe_level_inv_spacing_0 : array<vec4<f32>, i32(4)>,
    @align(16) hiz_0 : vec4<u32>,
    @align(16) sky_0 : array<vec4<f32>, i32(3)>,
};

@binding(0) @group(0) var<uniform> camera_0 : SsrParams_std140_0;
struct GpuProbe_std430_0
{
    @align(16) sh_r_0 : vec4<f32>,
    @align(16) sh_g_0 : vec4<f32>,
    @align(16) sh_b_0 : vec4<f32>,
};

@binding(4) @group(0) var<storage, read> probes_0 : array<GpuProbe_std430_0>;

@binding(12) @group(0) var probe_visibility_0 : texture_2d_array<f32>;

@binding(10) @group(0) var sky_prefilter_0 : texture_2d<f32>;

@binding(11) @group(0) var dfg_0 : texture_2d<f32>;

@binding(5) @group(0) var hiz_1_0 : texture_depth_2d;

@binding(6) @group(0) var hiz_2_0 : texture_depth_2d;

@binding(7) @group(0) var hiz_3_0 : texture_depth_2d;

@binding(8) @group(0) var hiz_4_0 : texture_depth_2d;

@binding(9) @group(0) var hiz_5_0 : texture_depth_2d;

@binding(2) @group(0) var scene_color_0 : texture_2d<f32>;

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

fn sharpness_of_0( roughness_0 : f32) -> f32
{
    return saturate(1.0f - roughness_0 / 0.5f);
}

fn depth_at_0( pixel_0 : vec2<i32>,  extent_0 : vec2<i32>) -> f32
{
    var _S9 : vec3<i32> = vec3<i32>(clamp(pixel_0, vec2<i32>(i32(0), i32(0)), extent_0 - vec2<i32>(i32(1), i32(1))), i32(0));
    return (textureLoad((scene_depth_0), ((_S9)).xy, ((_S9)).z));
}

fn unproject_z_0( depth_0 : f32) -> vec2<f32>
{
    return vec2<f32>(camera_0.inv_proj_0.data_0[i32(2)][i32(2)] * depth_0 + camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)] * depth_0 + camera_0.inv_proj_0.data_0[i32(3)][i32(3)]);
}

fn unproject_0( ndc_0 : vec2<f32>,  depth_1 : f32) -> vec4<f32>
{
    var depth_row_0 : vec2<f32> = unproject_z_0(depth_1);
    return vec4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)] * ndc_0.x + camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)] * ndc_0.y + camera_0.inv_proj_0.data_0[i32(3)][i32(1)], depth_row_0.x, depth_row_0.y);
}

fn view_position_0( pixel_1 : vec2<i32>,  depth_2 : f32,  extent_1 : vec2<f32>) -> vec3<f32>
{
    var view_0 : vec4<f32> = unproject_0(vec2<f32>((f32(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (f32(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_2);
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

fn probe_level_reach_0( world_position_0 : vec3<f32>,  origin_0 : vec3<f32>,  inv_spacing_0 : vec3<f32>,  last_0 : vec3<f32>) -> f32
{
    var reach_0 : f32 = 0.0f;
    var axis_0 : u32 = u32(0);
    for(;;)
    {
        if(axis_0 < u32(3))
        {
        }
        else
        {
            break;
        }
        var _S15 : u32 = axis_0;
        var _S16 : bool;
        if((last_0[axis_0]) == 0.0f)
        {
            _S16 = true;
        }
        else
        {
            _S16 = (inv_spacing_0[axis_0]) == 0.0f;
        }
        if(_S16)
        {
            axis_0 = axis_0 + u32(1);
            continue;
        }
        reach_0 = max(reach_0, abs(2.0f * ((world_position_0[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S15] - 1.0f));
        axis_0 = axis_0 + u32(1);
    }
    return reach_0;
}

fn probe_level_of_0( reach_1 : f32,  levels_0 : u32) -> vec2<f32>
{
    var level_0 : u32 = u32(0);
    for(;;)
    {
        var _S17 : u32 = level_0 + u32(1);
        if(_S17 < levels_0)
        {
        }
        else
        {
            break;
        }
        var _S18 : f32 = f32(level_0);
        var at_0 : f32 = reach_1 * exp2(- _S18);
        if(at_0 < 1.0f)
        {
            return vec2<f32>(_S18, saturate((1.0f - at_0) / 0.25f));
        }
        level_0 = _S17;
    }
    return vec2<f32>(f32(levels_0 - u32(1)), 1.0f);
}

fn probe_row_0( level_1 : u32,  cell_0 : vec3<u32>) -> u32
{
    return min(camera_0.probe_levels_0.y * level_1 + (cell_0.z * camera_0.probe_counts_0.y + cell_0.y) * camera_0.probe_counts_0.x + cell_0.x, max(camera_0.probe_counts_0.w, u32(1)) - u32(1));
}

fn sign_not_zero_0( value_0 : f32) -> f32
{
    var _S19 : f32;
    if(value_0 >= 0.0f)
    {
        _S19 = 1.0f;
    }
    else
    {
        _S19 = -1.0f;
    }
    return _S19;
}

fn oct_encode_0( direction_0 : vec3<f32>) -> vec2<f32>
{
    var _S20 : f32 = direction_0.y;
    var p_0 : vec2<f32> = direction_0.xz / vec2<f32>(max(abs(direction_0.x) + abs(_S20) + abs(direction_0.z), 9.99999968265522539e-21f));
    var p_1 : vec2<f32>;
    if(_S20 < 0.0f)
    {
        var _S21 : f32 = p_0.y;
        var _S22 : f32 = p_0.x;
        p_1 = vec2<f32>((1.0f - abs(_S21)) * sign_not_zero_0(_S22), (1.0f - abs(_S22)) * sign_not_zero_0(_S21));
    }
    else
    {
        p_1 = p_0;
    }
    return p_1;
}

fn probe_moments_0( index_1 : u32,  direction_1 : vec3<f32>) -> vec2<f32>
{
    var width_0 : u32;
    var height_0 : u32;
    var layers_0 : u32;
    {var dim = textureDimensions((probe_visibility_0));((width_0)) = dim.x;((height_0)) = dim.y;((layers_0)) = textureNumLayers((probe_visibility_0));};
    var _S23 : vec2<f32> = vec2<f32>(0.5f);
    var _S24 : vec2<f32> = vec2<f32>(1.0f);
    var scaled_0 : vec2<f32> = (oct_encode_0(direction_1) * _S23 + _S23) * vec2<f32>(16.0f) + _S24 - _S23;
    var _S25 : vec2<f32> = vec2<f32>(f32(width_0), f32(height_0)) - _S24;
    var low_0 : vec2<f32> = clamp(floor(scaled_0), vec2<f32>(0.0f, 0.0f), _S25);
    var high_0 : vec2<f32> = min(low_0 + _S24, _S25);
    var weight_0 : vec2<f32> = clamp(scaled_0 - low_0, vec2<f32>(0.0f), vec2<f32>(1.0f));
    var layer_0 : i32 = i32(min(index_1, max(layers_0, u32(1)) - u32(1)));
    var _S26 : i32 = i32(low_0.x);
    var _S27 : i32 = i32(low_0.y);
    var _S28 : vec4<i32> = vec4<i32>(_S26, _S27, layer_0, i32(0));
    var _S29 : i32 = i32(high_0.x);
    var _S30 : vec4<i32> = vec4<i32>(_S29, _S27, layer_0, i32(0));
    var _S31 : i32 = i32(high_0.y);
    var _S32 : vec4<i32> = vec4<i32>(_S26, _S31, layer_0, i32(0));
    var _S33 : vec4<i32> = vec4<i32>(_S29, _S31, layer_0, i32(0));
    var _S34 : vec2<f32> = vec2<f32>(weight_0.x);
    return mix(mix((textureLoad((probe_visibility_0), ((_S28)).xy, i32(((_S28)).z), ((_S28)).w)).xy, (textureLoad((probe_visibility_0), ((_S30)).xy, i32(((_S30)).z), ((_S30)).w)).xy, _S34), mix((textureLoad((probe_visibility_0), ((_S32)).xy, i32(((_S32)).z), ((_S32)).w)).xy, (textureLoad((probe_visibility_0), ((_S33)).xy, i32(((_S33)).z), ((_S33)).w)).xy, _S34), vec2<f32>(weight_0.y));
}

fn probe_chebyshev_0( index_2 : u32,  probe_position_0 : vec3<f32>,  world_position_1 : vec3<f32>,  normal_0 : vec3<f32>) -> f32
{
    var to_probe_0 : vec3<f32> = probe_position_0 - (world_position_1 + normal_0 * vec3<f32>(0.05000000074505806f));
    var to_surface_0 : f32 = length(to_probe_0);
    var moments_0 : vec2<f32> = probe_moments_0(index_2, (vec3<f32>(0) - to_probe_0));
    var _S35 : f32 = moments_0.x;
    var _S36 : f32 = max(moments_0.y - _S35 * _S35, 0.0f);
    var behind_0 : f32 = to_surface_0 - _S35;
    var bound_0 : f32 = _S36 / (_S36 + behind_0 * behind_0);
    var _S37 : f32;
    if(to_surface_0 <= _S35)
    {
        _S37 = 1.0f;
    }
    else
    {
        _S37 = bound_0 * bound_0 * bound_0;
    }
    return _S37;
}

fn probe_weight_0( index_3 : u32,  probe_position_1 : vec3<f32>,  world_position_2 : vec3<f32>,  normal_1 : vec3<f32>) -> f32
{
    return max(probe_chebyshev_0(index_3, probe_position_1, world_position_2, normal_1), 0.00009999999747379f);
}

struct GpuProbe_0
{
     sh_r_0 : vec4<f32>,
     sh_g_0 : vec4<f32>,
     sh_b_0 : vec4<f32>,
};

struct WeightedProbe_0
{
     sh_0 : GpuProbe_0,
     weight_1 : f32,
};

fn probe_corner_0( level_2 : u32,  cell_1 : vec3<u32>,  origin_1 : vec3<f32>,  spacing_0 : vec3<f32>,  world_position_3 : vec3<f32>,  normal_2 : vec3<f32>) -> WeightedProbe_0
{
    var row_0 : u32 = probe_row_0(level_2, cell_1);
    var stored_0 : GpuProbe_std430_0 = probes_0[row_0];
    var weight_2 : f32 = probe_weight_0(row_0, origin_1 + vec3<f32>(cell_1) * spacing_0, world_position_3, normal_2);
    var corner_0 : WeightedProbe_0;
    var _S38 : vec4<f32> = vec4<f32>(weight_2);
    corner_0.sh_0.sh_r_0 = stored_0.sh_r_0 * _S38;
    corner_0.sh_0.sh_g_0 = stored_0.sh_g_0 * _S38;
    corner_0.sh_0.sh_b_0 = stored_0.sh_b_0 * _S38;
    corner_0.weight_1 = weight_2;
    return corner_0;
}

fn lerp_probe_0( a_0 : WeightedProbe_0,  b_0 : WeightedProbe_0,  t_0 : f32) -> WeightedProbe_0
{
    var blended_0 : WeightedProbe_0;
    var _S39 : vec4<f32> = vec4<f32>(t_0);
    blended_0.sh_0.sh_r_0 = mix(a_0.sh_0.sh_r_0, b_0.sh_0.sh_r_0, _S39);
    blended_0.sh_0.sh_g_0 = mix(a_0.sh_0.sh_g_0, b_0.sh_0.sh_g_0, _S39);
    blended_0.sh_0.sh_b_0 = mix(a_0.sh_0.sh_b_0, b_0.sh_0.sh_b_0, _S39);
    blended_0.weight_1 = mix(a_0.weight_1, b_0.weight_1, t_0);
    return blended_0;
}

fn probe_level_environment_0( level_3 : u32,  world_position_4 : vec3<f32>,  normal_3 : vec3<f32>,  direction_2 : vec3<f32>) -> vec3<f32>
{
    var _S40 : vec3<f32> = vec3<f32>(1.0f);
    const _S41 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var last_1 : vec3<f32> = max(vec3<f32>(camera_0.probe_counts_0.xyz) - _S40, _S41);
    var origin_2 : vec3<f32> = camera_0.probe_level_origin_0[level_3].xyz;
    var inv_0 : vec3<f32> = camera_0.probe_level_inv_spacing_0[level_3].xyz;
    var grid_0 : vec3<f32> = clamp((world_position_4 - origin_2) * inv_0, _S41, last_1);
    var base_0 : vec3<f32> = floor(grid_0);
    var f_0 : vec3<f32> = grid_0 - base_0;
    var _S42 : vec3<u32> = vec3<u32>(base_0);
    var _S43 : vec3<u32> = vec3<u32>(min(base_0 + _S40, last_1));
    var _S44 : f32 = inv_0.x;
    var _S45 : f32;
    if(_S44 != 0.0f)
    {
        _S45 = 1.0f / _S44;
    }
    else
    {
        _S45 = 0.0f;
    }
    var _S46 : f32 = inv_0.y;
    var _S47 : f32;
    if(_S46 != 0.0f)
    {
        _S47 = 1.0f / _S46;
    }
    else
    {
        _S47 = 0.0f;
    }
    var _S48 : f32 = inv_0.z;
    var _S49 : f32;
    if(_S48 != 0.0f)
    {
        _S49 = 1.0f / _S48;
    }
    else
    {
        _S49 = 0.0f;
    }
    var spacing_1 : vec3<f32> = vec3<f32>(_S45, _S47, _S49);
    var _S50 : u32 = _S42.x;
    var _S51 : u32 = _S42.y;
    var _S52 : u32 = _S42.z;
    var _S53 : u32 = _S43.x;
    var _S54 : f32 = f_0.x;
    var _S55 : u32 = _S43.y;
    var _S56 : u32 = _S43.z;
    var _S57 : f32 = f_0.y;
    var cell_2 : WeightedProbe_0 = lerp_probe_0(lerp_probe_0(lerp_probe_0(probe_corner_0(level_3, vec3<u32>(_S50, _S51, _S52), origin_2, spacing_1, world_position_4, normal_3), probe_corner_0(level_3, vec3<u32>(_S53, _S51, _S52), origin_2, spacing_1, world_position_4, normal_3), _S54), lerp_probe_0(probe_corner_0(level_3, vec3<u32>(_S50, _S55, _S52), origin_2, spacing_1, world_position_4, normal_3), probe_corner_0(level_3, vec3<u32>(_S53, _S55, _S52), origin_2, spacing_1, world_position_4, normal_3), _S54), _S57), lerp_probe_0(lerp_probe_0(probe_corner_0(level_3, vec3<u32>(_S50, _S51, _S56), origin_2, spacing_1, world_position_4, normal_3), probe_corner_0(level_3, vec3<u32>(_S53, _S51, _S56), origin_2, spacing_1, world_position_4, normal_3), _S54), lerp_probe_0(probe_corner_0(level_3, vec3<u32>(_S50, _S55, _S56), origin_2, spacing_1, world_position_4, normal_3), probe_corner_0(level_3, vec3<u32>(_S53, _S55, _S56), origin_2, spacing_1, world_position_4, normal_3), _S54), _S57), f_0.z);
    var _S58 : vec3<f32> = vec3<f32>(2.09439516067504883f);
    return max(vec3<f32>(dot(cell_2.sh_0.sh_r_0.xyz / _S58, direction_2) + cell_2.sh_0.sh_r_0.w / 3.14159274101257324f, dot(cell_2.sh_0.sh_g_0.xyz / _S58, direction_2) + cell_2.sh_0.sh_g_0.w / 3.14159274101257324f, dot(cell_2.sh_0.sh_b_0.xyz / _S58, direction_2) + cell_2.sh_0.sh_b_0.w / 3.14159274101257324f) / vec3<f32>(cell_2.weight_1), _S41);
}

fn probe_environment_0( world_position_5 : vec3<f32>,  normal_4 : vec3<f32>,  direction_3 : vec3<f32>) -> vec3<f32>
{
    var pick_0 : vec2<f32> = probe_level_of_0(probe_level_reach_0(world_position_5, camera_0.probe_level_origin_0[i32(0)].xyz, camera_0.probe_level_inv_spacing_0[i32(0)].xyz, max(vec3<f32>(camera_0.probe_counts_0.xyz) - vec3<f32>(1.0f), vec3<f32>(0.0f, 0.0f, 0.0f))), clamp(camera_0.probe_levels_0.x, u32(1), u32(4)));
    var level_4 : u32 = u32(pick_0.x);
    var share_0 : f32 = pick_0.y;
    var fine_0 : vec3<f32> = probe_level_environment_0(level_4, world_position_5, normal_4, direction_3);
    if(share_0 >= 1.0f)
    {
        return fine_0;
    }
    return probe_level_environment_0(level_4 + u32(1), world_position_5, normal_4, direction_3) * vec3<f32>((1.0f - share_0)) + fine_0 * vec3<f32>(share_0);
}

fn decode_fixed_pair_0( texel_0 : vec4<f32>) -> vec2<f32>
{
    return vec2<f32>(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / vec2<f32>(65535.0f);
}

fn fixed_pair_at_0( table_0 : texture_2d<f32>,  at_1 : vec2<f32>) -> vec2<f32>
{
    var width_1 : u32;
    var height_1 : u32;
    {var dim = textureDimensions((table_0));((width_1)) = dim.x;((height_1)) = dim.y;};
    var extent_3 : vec2<f32> = vec2<f32>(f32(width_1), f32(height_1));
    var scaled_1 : vec2<f32> = saturate(at_1) * extent_3 - vec2<f32>(0.5f);
    var _S59 : vec2<f32> = vec2<f32>(1.0f);
    var _S60 : vec2<f32> = extent_3 - _S59;
    var low_1 : vec2<f32> = clamp(floor(scaled_1), vec2<f32>(0.0f, 0.0f), _S60);
    var weight_3 : vec2<f32> = clamp(scaled_1 - low_1, vec2<f32>(0.0f), vec2<f32>(1.0f));
    var _S61 : vec2<i32> = vec2<i32>(low_1);
    var _S62 : vec2<i32> = vec2<i32>(min(low_1 + _S59, _S60));
    var _S63 : i32 = _S61.x;
    var _S64 : i32 = _S61.y;
    var _S65 : vec3<i32> = vec3<i32>(_S63, _S64, i32(0));
    var _S66 : i32 = _S62.x;
    var _S67 : vec3<i32> = vec3<i32>(_S66, _S64, i32(0));
    var _S68 : vec2<f32> = vec2<f32>(weight_3.x);
    var _S69 : i32 = _S62.y;
    var _S70 : vec3<i32> = vec3<i32>(_S63, _S69, i32(0));
    var _S71 : vec3<i32> = vec3<i32>(_S66, _S69, i32(0));
    return mix(mix(decode_fixed_pair_0((textureLoad((table_0), ((_S65)).xy, ((_S65)).z))), decode_fixed_pair_0((textureLoad((table_0), ((_S67)).xy, ((_S67)).z))), _S68), mix(decode_fixed_pair_0((textureLoad((table_0), ((_S70)).xy, ((_S70)).z))), decode_fixed_pair_0((textureLoad((table_0), ((_S71)).xy, ((_S71)).z))), _S68), vec2<f32>(weight_3.y));
}

fn sky_prefilter_at_0( up_1 : f32,  roughness_1 : f32) -> vec2<f32>
{
    return fixed_pair_at_0(sky_prefilter_0, vec2<f32>(up_1, roughness_1));
}

fn sky_prefiltered_0( direction_4 : vec3<f32>,  roughness_2 : f32) -> vec3<f32>
{
    var up_2 : f32 = clamp(direction_4.y, -1.0f, 1.0f);
    var weights_0 : vec2<f32> = sky_prefilter_at_0(abs(up_2), roughness_2);
    var _S72 : bool = up_2 >= 0.0f;
    var far_0 : vec3<f32>;
    if(_S72)
    {
        far_0 = camera_0.sky_0[i32(0)].xyz;
    }
    else
    {
        far_0 = camera_0.sky_0[i32(2)].xyz;
    }
    var opposite_0 : vec3<f32>;
    if(_S72)
    {
        opposite_0 = camera_0.sky_0[i32(2)].xyz;
    }
    else
    {
        opposite_0 = camera_0.sky_0[i32(0)].xyz;
    }
    var _S73 : f32 = weights_0.x;
    var _S74 : f32 = weights_0.y;
    return camera_0.sky_0[i32(1)].xyz * vec3<f32>((1.0f - _S73 - _S74)) + far_0 * vec3<f32>(_S73) + opposite_0 * vec3<f32>(_S74);
}

fn dfg_at_0( n_dot_v_0 : f32,  roughness_3 : f32) -> vec2<f32>
{
    return fixed_pair_at_0(dfg_0, vec2<f32>(n_dot_v_0, roughness_3));
}

fn pixel_of_0( ndc_1 : vec2<f32>,  size_1 : vec2<f32>) -> vec2<f32>
{
    return vec2<f32>((ndc_1.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_1.y * 0.5f) * size_1.y);
}

fn ndc_of_0( at_2 : vec2<f32>,  size_2 : vec2<f32>) -> vec2<f32>
{
    return vec2<f32>(at_2.x / size_2.x * 2.0f - 1.0f, 1.0f - at_2.y / size_2.y * 2.0f);
}

fn cell_exit_0( at_3 : vec2<f32>,  forward_0 : vec2<f32>,  size_3 : f32,  reach_2 : f32) -> f32
{
    var _S75 : f32 = forward_0.x;
    var _S76 : bool = _S75 > 0.0f;
    var along_x_0 : f32;
    if(_S76)
    {
        along_x_0 = (floor(at_3.x / size_3) + 1.0f) * size_3;
    }
    else
    {
        along_x_0 = floor(at_3.x / size_3) * size_3;
    }
    var _S77 : f32 = forward_0.y;
    var _S78 : bool = _S77 > 0.0f;
    var along_y_0 : f32;
    if(_S78)
    {
        along_y_0 = (floor(at_3.y / size_3) + 1.0f) * size_3;
    }
    else
    {
        along_y_0 = floor(at_3.y / size_3) * size_3;
    }
    var nudge_0 : f32 = size_3 * 0.00390625f;
    var _S79 : f32;
    if((abs(_S75)) < 9.99999997475242708e-07f)
    {
        along_x_0 = reach_2;
    }
    else
    {
        if(_S76)
        {
            _S79 = nudge_0;
        }
        else
        {
            _S79 = - nudge_0;
        }
        along_x_0 = (along_x_0 + _S79 - at_3.x) / _S75;
    }
    if((abs(_S77)) < 9.99999997475242708e-07f)
    {
        along_y_0 = reach_2;
    }
    else
    {
        if(_S78)
        {
            _S79 = nudge_0;
        }
        else
        {
            _S79 = - nudge_0;
        }
        along_y_0 = (along_y_0 + _S79 - at_3.y) / _S77;
    }
    return max(min(along_x_0, along_y_0), nudge_0);
}

fn hiz_at_0( level_5 : u32,  texel_1 : vec2<i32>,  extent_4 : vec2<i32>) -> f32
{
    const _S80 : vec2<i32> = vec2<i32>(i32(0), i32(0));
    var at_4 : vec3<i32> = vec3<i32>(clamp(texel_1, _S80, max(extent_4 - vec2<i32>(i32(1), i32(1)), _S80)), i32(0));
    switch(level_5)
    {
    case u32(0):
        {
            return (textureLoad((scene_depth_0), ((at_4)).xy, ((at_4)).z));
        }
    case u32(1):
        {
            return (textureLoad((hiz_1_0), ((at_4)).xy, ((at_4)).z));
        }
    case u32(2):
        {
            return (textureLoad((hiz_2_0), ((at_4)).xy, ((at_4)).z));
        }
    case u32(3):
        {
            return (textureLoad((hiz_3_0), ((at_4)).xy, ((at_4)).z));
        }
    case u32(4):
        {
            return (textureLoad((hiz_4_0), ((at_4)).xy, ((at_4)).z));
        }
    default :
        {
            return (textureLoad((hiz_5_0), ((at_4)).xy, ((at_4)).z));
        }
    }
}

fn view_z_of_0( depth_3 : f32) -> f32
{
    var view_1 : vec2<f32> = unproject_z_0(depth_3);
    return view_1.x / view_1.y;
}

fn thickness_at_0( advance_0 : f32,  depth_4 : f32) -> f32
{
    return max(advance_0, abs(depth_4) * 0.01999999955296516f);
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
fn fragmentMain( _S81 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var reflection_0 : vec3<f32>;
    var width_2 : u32;
    var height_2 : u32;
    {var dim = textureDimensions((scene_depth_0));((width_2)) = dim.x;((height_2)) = dim.y;};
    var _S82 : i32 = i32(width_2);
    var _S83 : i32 = i32(height_2);
    var extent_5 : vec2<i32> = vec2<i32>(_S82, _S83);
    var _S84 : f32 = f32(width_2);
    var _S85 : f32 = f32(height_2);
    var size_4 : vec2<f32> = vec2<f32>(_S84, _S85);
    var _S86 : vec2<i32> = vec2<i32>(position_1.xy);
    const NOTHING_0 : vec4<f32> = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
    var _S87 : vec3<i32> = vec3<i32>(_S86, i32(0));
    var surface_0 : vec4<f32> = (textureLoad((reflectivity_0), ((_S87)).xy, ((_S87)).z));
    var _S88 : f32 = surface_0.w;
    var sharpness_0 : f32 = sharpness_of_0(_S88);
    var depth_5 : f32 = depth_at_0(_S86, extent_5);
    if(depth_5 <= 0.0f)
    {
        var _S89 : pixelOutput_0 = pixelOutput_0( NOTHING_0 );
        return _S89;
    }
    var origin_3 : vec3<f32> = view_position_0(_S86, depth_5, size_4);
    var normal_5 : vec3<f32> = normal_at_0(_S86, origin_3, extent_5, size_4);
    var towards_0 : vec3<f32> = normalize(origin_3);
    var ray_0 : vec3<f32> = reflect(towards_0, normal_5);
    var _S90 : vec4<f32> = vec4<f32>(ray_0, 0.0f);
    var reflection_direction_0 : vec3<f32> = normalize((((_S90) * (mat4x4<f32>(camera_0.inv_view_0.data_0[i32(0)][i32(0)], camera_0.inv_view_0.data_0[i32(1)][i32(0)], camera_0.inv_view_0.data_0[i32(2)][i32(0)], camera_0.inv_view_0.data_0[i32(3)][i32(0)], camera_0.inv_view_0.data_0[i32(0)][i32(1)], camera_0.inv_view_0.data_0[i32(1)][i32(1)], camera_0.inv_view_0.data_0[i32(2)][i32(1)], camera_0.inv_view_0.data_0[i32(3)][i32(1)], camera_0.inv_view_0.data_0[i32(0)][i32(2)], camera_0.inv_view_0.data_0[i32(1)][i32(2)], camera_0.inv_view_0.data_0[i32(2)][i32(2)], camera_0.inv_view_0.data_0[i32(3)][i32(2)], camera_0.inv_view_0.data_0[i32(0)][i32(3)], camera_0.inv_view_0.data_0[i32(1)][i32(3)], camera_0.inv_view_0.data_0[i32(2)][i32(3)], camera_0.inv_view_0.data_0[i32(3)][i32(3)])))).xyz);
    var environment_0 : vec3<f32> = probe_environment_0((((vec4<f32>(origin_3, 1.0f)) * (mat4x4<f32>(camera_0.inv_view_0.data_0[i32(0)][i32(0)], camera_0.inv_view_0.data_0[i32(1)][i32(0)], camera_0.inv_view_0.data_0[i32(2)][i32(0)], camera_0.inv_view_0.data_0[i32(3)][i32(0)], camera_0.inv_view_0.data_0[i32(0)][i32(1)], camera_0.inv_view_0.data_0[i32(1)][i32(1)], camera_0.inv_view_0.data_0[i32(2)][i32(1)], camera_0.inv_view_0.data_0[i32(3)][i32(1)], camera_0.inv_view_0.data_0[i32(0)][i32(2)], camera_0.inv_view_0.data_0[i32(1)][i32(2)], camera_0.inv_view_0.data_0[i32(2)][i32(2)], camera_0.inv_view_0.data_0[i32(3)][i32(2)], camera_0.inv_view_0.data_0[i32(0)][i32(3)], camera_0.inv_view_0.data_0[i32(1)][i32(3)], camera_0.inv_view_0.data_0[i32(2)][i32(3)], camera_0.inv_view_0.data_0[i32(3)][i32(3)])))).xyz, normalize((((vec4<f32>(normal_5, 0.0f)) * (mat4x4<f32>(camera_0.inv_view_0.data_0[i32(0)][i32(0)], camera_0.inv_view_0.data_0[i32(1)][i32(0)], camera_0.inv_view_0.data_0[i32(2)][i32(0)], camera_0.inv_view_0.data_0[i32(3)][i32(0)], camera_0.inv_view_0.data_0[i32(0)][i32(1)], camera_0.inv_view_0.data_0[i32(1)][i32(1)], camera_0.inv_view_0.data_0[i32(2)][i32(1)], camera_0.inv_view_0.data_0[i32(3)][i32(1)], camera_0.inv_view_0.data_0[i32(0)][i32(2)], camera_0.inv_view_0.data_0[i32(1)][i32(2)], camera_0.inv_view_0.data_0[i32(2)][i32(2)], camera_0.inv_view_0.data_0[i32(3)][i32(2)], camera_0.inv_view_0.data_0[i32(0)][i32(3)], camera_0.inv_view_0.data_0[i32(1)][i32(3)], camera_0.inv_view_0.data_0[i32(2)][i32(3)], camera_0.inv_view_0.data_0[i32(3)][i32(3)])))).xyz), reflection_direction_0) + sky_prefiltered_0(reflection_direction_0, _S88);
    var _S91 : vec3<f32> = (vec3<f32>(0) - towards_0);
    var dfg_terms_0 : vec2<f32> = dfg_at_0(saturate(dot(normal_5, _S91)), _S88);
    var env_brdf_0 : vec3<f32> = surface_0.xyz * vec3<f32>(dfg_terms_0.x) + vec3<f32>(dfg_terms_0.y);
    if(sharpness_0 <= 0.0f)
    {
        var _S92 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * env_brdf_0, 0.0f) );
        return _S92;
    }
    var _S93 : f32 = saturate((1.0f - dot(ray_0, _S91)) / 0.05000000074505806f);
    var _S94 : f32 = origin_3.z;
    var start_0 : vec3<f32> = origin_3 + normal_5 * vec3<f32>((abs(_S94) * 0.00499999988824129f));
    var clip_start_0 : vec4<f32> = (((vec4<f32>(start_0, 1.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var clip_ray_0 : vec4<f32> = (((_S90) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var _S95 : f32 = clip_start_0.w;
    if(_S95 <= 0.0f)
    {
        var _S96 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * env_brdf_0, sharpness_0) );
        return _S96;
    }
    var _S97 : vec2<f32> = clip_start_0.xy;
    var _S98 : vec2<f32> = vec2<f32>(_S95);
    var at_start_0 : vec2<f32> = pixel_of_0(_S97 / _S98, size_4);
    var _S99 : vec2<f32> = clip_ray_0.xy;
    var _S100 : f32 = clip_ray_0.w;
    var _S101 : vec2<f32> = vec2<f32>(_S100);
    var ndc_rate_0 : vec2<f32> = (_S99 * _S98 - _S97 * _S101) / vec2<f32>((_S95 * _S95));
    var screen_rate_0 : vec2<f32> = vec2<f32>(ndc_rate_0.x * 0.5f * _S84, - ndc_rate_0.y * 0.5f * _S85);
    var rate_0 : f32 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {
        var _S102 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * env_brdf_0, sharpness_0) );
        return _S102;
    }
    var forward_1 : vec2<f32> = screen_rate_0 / vec2<f32>(rate_0);
    var reach_3 : f32 = 0.75f * min(_S84, _S85);
    var _S103 : f32 = forward_1.x;
    var travel_0 : f32;
    if(_S103 > 0.0f)
    {
        travel_0 = min(reach_3, (_S84 - 1.0f - at_start_0.x) / _S103);
    }
    else
    {
        if(_S103 < 0.0f)
        {
            travel_0 = min(reach_3, - at_start_0.x / _S103);
        }
        else
        {
            travel_0 = reach_3;
        }
    }
    var _S104 : f32 = forward_1.y;
    if(_S104 > 0.0f)
    {
        travel_0 = min(travel_0, (_S85 - 1.0f - at_start_0.y) / _S104);
    }
    else
    {
        if(_S104 < 0.0f)
        {
            travel_0 = min(travel_0, - at_start_0.y / _S104);
        }
    }
    if(_S100 > 0.0f)
    {
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S99 / _S101, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));
    }
    else
    {
        if(_S100 < 0.0f)
        {
            var on_near_0 : vec4<f32> = (((vec4<f32>(0.0f, 0.0f, 1.0f, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
            var clip_near_0 : vec4<f32> = clip_start_0 + clip_ray_0 * vec4<f32>(((- on_near_0.z / on_near_0.w - _S95) / _S100));
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / vec2<f32>(clip_near_0.w), size_4) - at_start_0, forward_1), 0.0f));
        }
    }
    var _S105 : f32 = max(travel_0, 0.0f);
    if(_S105 <= 0.00390625f)
    {
        var _S106 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * env_brdf_0, sharpness_0) );
        return _S106;
    }
    var ndc_end_0 : vec2<f32> = ndc_of_0(at_start_0 + forward_1 * vec2<f32>(_S105), size_4);
    var when_end_0 : f32;
    if((abs(_S103)) >= (abs(_S104)))
    {
        var _S107 : f32 = ndc_end_0.x;
        when_end_0 = (_S107 * _S95 - clip_start_0.x) / (clip_ray_0.x - _S107 * _S100);
    }
    else
    {
        var _S108 : f32 = ndc_end_0.y;
        when_end_0 = (_S108 * _S95 - clip_start_0.y) / (clip_ray_0.y - _S108 * _S100);
    }
    var _S109 : bool;
    if(!(when_end_0 > 0.0f))
    {
        _S109 = true;
    }
    else
    {
        _S109 = !isfinite_0(when_end_0);
    }
    if(_S109)
    {
        var _S110 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * env_brdf_0, sharpness_0) );
        return _S110;
    }
    var inverse_w_start_0 : f32 = 1.0f / _S95;
    var inverse_w_end_0 : f32 = 1.0f / (_S95 + when_end_0 * _S100);
    var _S111 : f32 = start_0.z;
    var _S112 : f32 = _S111 * inverse_w_start_0;
    var _S113 : f32 = (_S111 + when_end_0 * ray_0.z) * inverse_w_end_0;
    var _S114 : vec3<f32> = environment_0 * env_brdf_0;
    var _S115 : u32 = min(camera_0.hiz_0.x, u32(5));
    var _S116 : f32 = _S111 - _S94;
    var at_travel_0 : f32 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S105), _S105);
    var previous_gap_0 : f32 = _S116;
    var entry_z_0 : f32 = _S111;
    var step_0 : u32 = u32(0);
    var level_6 : u32 = u32(0);
    for(;;)
    {
        if(step_0 < u32(96))
        {
        }
        else
        {
            reflection_0 = _S114;
            break;
        }
        var cell_3 : f32 = f32((u32(1) << (level_6)));
        var at_5 : vec2<f32> = at_start_0 + forward_1 * vec2<f32>(at_travel_0);
        var _S117 : f32 = min(at_travel_0 + cell_exit_0(at_5, forward_1, cell_3, _S105), _S105);
        var exit_at_0 : vec2<f32> = at_start_0 + forward_1 * vec2<f32>(_S117);
        var along_0 : f32 = _S117 / _S105;
        var exit_z_0 : f32 = mix(_S112, _S113, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);
        var cell_depth_0 : f32 = hiz_at_0(level_6, vec2<i32>(floor(at_5 / vec2<f32>(cell_3))), vec2<i32>((_S82 >> (level_6)), (_S83 >> (level_6))));
        var gap_0 : f32;
        if(cell_depth_0 <= 0.0f)
        {
            gap_0 = 1.0f;
        }
        else
        {
            gap_0 = exit_z_0 - view_z_of_0(cell_depth_0);
        }
        var _S118 : bool = !(gap_0 > 0.0f);
        if(_S118)
        {
            _S109 = level_6 > u32(0);
        }
        else
        {
            _S109 = false;
        }
        if(_S109)
        {
            level_6 = level_6 - u32(1);
            step_0 = step_0 + u32(1);
            continue;
        }
        var _S119 : bool;
        if(_S118)
        {
            _S119 = previous_gap_0 > 0.0f;
        }
        else
        {
            _S119 = false;
        }
        if(_S119)
        {
            var behind_1 : f32 = - gap_0;
            var thickness_0 : f32 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_1 <= thickness_0)
            {
                var hit_at_0 : vec2<f32> = mix(at_5, exit_at_0, vec2<f32>((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))));
                var hit_ndc_0 : vec2<f32> = ndc_of_0(hit_at_0, size_4);
                var confidence_0 : f32 = sharpness_0 * _S93 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S117 / reach_3) / 0.25f) * saturate(1.0f - behind_1 / thickness_0);
                var _S120 : vec3<i32> = vec3<i32>(clamp(vec2<i32>(hit_at_0), vec2<i32>(i32(0), i32(0)), extent_5 - vec2<i32>(i32(1), i32(1))), i32(0));
                reflection_0 = (textureLoad((scene_color_0), ((_S120)).xy, ((_S120)).z)).xyz * env_brdf_0 * vec3<f32>(confidence_0) + _S114 * vec3<f32>((1.0f - confidence_0));
                break;
            }
        }
        if(_S117 >= _S105)
        {
            reflection_0 = _S114;
            break;
        }
        var _S121 : u32 = min(level_6 + u32(1), _S115);
        at_travel_0 = _S117;
        previous_gap_0 = gap_0;
        entry_z_0 = exit_z_0;
        level_6 = _S121;
        step_0 = step_0 + u32(1);
    }
    var _S122 : pixelOutput_0 = pixelOutput_0( vec4<f32>(reflection_0, sharpness_0) );
    return _S122;
}

