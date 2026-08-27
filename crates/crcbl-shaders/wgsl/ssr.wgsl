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
    @align(16) probe_origin_0 : vec4<f32>,
    @align(16) probe_inv_spacing_0 : vec4<f32>,
    @align(16) probe_counts_0 : vec4<u32>,
    @align(16) hiz_0 : vec4<u32>,
};

@binding(0) @group(0) var<uniform> camera_0 : SsrParams_std140_0;
struct GpuProbe_std430_0
{
    @align(16) sh_r_0 : vec4<f32>,
    @align(16) sh_g_0 : vec4<f32>,
    @align(16) sh_b_0 : vec4<f32>,
};

@binding(4) @group(0) var<storage, read> probes_0 : array<GpuProbe_std430_0>;

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

struct GpuProbe_0
{
     sh_r_0 : vec4<f32>,
     sh_g_0 : vec4<f32>,
     sh_b_0 : vec4<f32>,
};

fn probe_environment_0( world_position_0 : vec3<f32>,  direction_0 : vec3<f32>) -> vec3<f32>
{
    var _S15 : vec3<f32> = vec3<f32>(1.0f);
    const _S16 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var last_0 : vec3<f32> = max(vec3<f32>(camera_0.probe_counts_0.xyz) - _S15, _S16);
    var grid_0 : vec3<f32> = clamp((world_position_0 - camera_0.probe_origin_0.xyz) * camera_0.probe_inv_spacing_0.xyz, _S16, last_0);
    var base_0 : vec3<f32> = floor(grid_0);
    var f_0 : vec3<f32> = grid_0 - base_0;
    var _S17 : vec3<u32> = vec3<u32>(base_0);
    var _S18 : vec3<u32> = vec3<u32>(min(base_0 + _S15, last_0));
    var total_0 : u32 = max(camera_0.probe_counts_0.w, u32(1)) - u32(1);
    var _S19 : u32 = _S17.z;
    var _S20 : u32 = _S17.y;
    var _S21 : u32 = _S17.x;
    var _S22 : u32 = _S18.x;
    var _S23 : u32 = _S18.y;
    var _S24 : u32 = _S18.z;
    var x00_0 : GpuProbe_std430_0 = probes_0[min((_S19 * camera_0.probe_counts_0.y + _S20) * camera_0.probe_counts_0.x + _S21, total_0)];
    var x10_0 : GpuProbe_std430_0 = probes_0[min((_S19 * camera_0.probe_counts_0.y + _S23) * camera_0.probe_counts_0.x + _S21, total_0)];
    var x01_0 : GpuProbe_std430_0 = probes_0[min((_S24 * camera_0.probe_counts_0.y + _S20) * camera_0.probe_counts_0.x + _S21, total_0)];
    var x11_0 : GpuProbe_std430_0 = probes_0[min((_S24 * camera_0.probe_counts_0.y + _S23) * camera_0.probe_counts_0.x + _S21, total_0)];
    var y00_0 : GpuProbe_std430_0 = probes_0[min((_S19 * camera_0.probe_counts_0.y + _S20) * camera_0.probe_counts_0.x + _S22, total_0)];
    var y10_0 : GpuProbe_std430_0 = probes_0[min((_S19 * camera_0.probe_counts_0.y + _S23) * camera_0.probe_counts_0.x + _S22, total_0)];
    var y01_0 : GpuProbe_std430_0 = probes_0[min((_S24 * camera_0.probe_counts_0.y + _S20) * camera_0.probe_counts_0.x + _S22, total_0)];
    var y11_0 : GpuProbe_std430_0 = probes_0[min((_S24 * camera_0.probe_counts_0.y + _S23) * camera_0.probe_counts_0.x + _S22, total_0)];
    var z0_0 : GpuProbe_0;
    var _S25 : vec4<f32> = vec4<f32>(f_0.x);
    var _S26 : vec4<f32> = vec4<f32>(f_0.y);
    var _S27 : vec4<f32> = mix(mix(x00_0.sh_r_0, y00_0.sh_r_0, _S25), mix(x10_0.sh_r_0, y10_0.sh_r_0, _S25), _S26);
    z0_0.sh_r_0 = _S27;
    var _S28 : vec4<f32> = mix(mix(x00_0.sh_g_0, y00_0.sh_g_0, _S25), mix(x10_0.sh_g_0, y10_0.sh_g_0, _S25), _S26);
    z0_0.sh_g_0 = _S28;
    var _S29 : vec4<f32> = mix(mix(x00_0.sh_b_0, y00_0.sh_b_0, _S25), mix(x10_0.sh_b_0, y10_0.sh_b_0, _S25), _S26);
    z0_0.sh_b_0 = _S29;
    var z1_0 : GpuProbe_0;
    var _S30 : vec4<f32> = mix(mix(x01_0.sh_r_0, y01_0.sh_r_0, _S25), mix(x11_0.sh_r_0, y11_0.sh_r_0, _S25), _S26);
    z1_0.sh_r_0 = _S30;
    var _S31 : vec4<f32> = mix(mix(x01_0.sh_g_0, y01_0.sh_g_0, _S25), mix(x11_0.sh_g_0, y11_0.sh_g_0, _S25), _S26);
    z1_0.sh_g_0 = _S31;
    var _S32 : vec4<f32> = mix(mix(x01_0.sh_b_0, y01_0.sh_b_0, _S25), mix(x11_0.sh_b_0, y11_0.sh_b_0, _S25), _S26);
    z1_0.sh_b_0 = _S32;
    var cell_0 : GpuProbe_0;
    var _S33 : vec4<f32> = vec4<f32>(f_0.z);
    var _S34 : vec4<f32> = mix(_S27, _S30, _S33);
    cell_0.sh_r_0 = _S34;
    var _S35 : vec4<f32> = mix(_S28, _S31, _S33);
    cell_0.sh_g_0 = _S35;
    var _S36 : vec4<f32> = mix(_S29, _S32, _S33);
    cell_0.sh_b_0 = _S36;
    var _S37 : vec3<f32> = vec3<f32>(2.09439516067504883f);
    return max(vec3<f32>(dot(_S34.xyz / _S37, direction_0) + _S34.w / 3.14159274101257324f, dot(_S35.xyz / _S37, direction_0) + _S35.w / 3.14159274101257324f, dot(_S36.xyz / _S37, direction_0) + _S36.w / 3.14159274101257324f), _S16);
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
    var _S38 : f32 = forward_0.x;
    var _S39 : bool = _S38 > 0.0f;
    var along_x_0 : f32;
    if(_S39)
    {
        along_x_0 = (floor(at_1.x / size_3) + 1.0f) * size_3;
    }
    else
    {
        along_x_0 = floor(at_1.x / size_3) * size_3;
    }
    var _S40 : f32 = forward_0.y;
    var _S41 : bool = _S40 > 0.0f;
    var along_y_0 : f32;
    if(_S41)
    {
        along_y_0 = (floor(at_1.y / size_3) + 1.0f) * size_3;
    }
    else
    {
        along_y_0 = floor(at_1.y / size_3) * size_3;
    }
    var nudge_0 : f32 = size_3 * 0.00390625f;
    var _S42 : f32;
    if((abs(_S38)) < 9.99999997475242708e-07f)
    {
        along_x_0 = reach_0;
    }
    else
    {
        if(_S39)
        {
            _S42 = nudge_0;
        }
        else
        {
            _S42 = - nudge_0;
        }
        along_x_0 = (along_x_0 + _S42 - at_1.x) / _S38;
    }
    if((abs(_S40)) < 9.99999997475242708e-07f)
    {
        along_y_0 = reach_0;
    }
    else
    {
        if(_S41)
        {
            _S42 = nudge_0;
        }
        else
        {
            _S42 = - nudge_0;
        }
        along_y_0 = (along_y_0 + _S42 - at_1.y) / _S40;
    }
    return max(min(along_x_0, along_y_0), nudge_0);
}

fn hiz_at_0( level_0 : u32,  texel_0 : vec2<i32>,  extent_3 : vec2<i32>) -> f32
{
    const _S43 : vec2<i32> = vec2<i32>(i32(0), i32(0));
    var at_2 : vec3<i32> = vec3<i32>(clamp(texel_0, _S43, max(extent_3 - vec2<i32>(i32(1), i32(1)), _S43)), i32(0));
    switch(level_0)
    {
    case u32(0):
        {
            return (textureLoad((scene_depth_0), ((at_2)).xy, ((at_2)).z));
        }
    case u32(1):
        {
            return (textureLoad((hiz_1_0), ((at_2)).xy, ((at_2)).z));
        }
    case u32(2):
        {
            return (textureLoad((hiz_2_0), ((at_2)).xy, ((at_2)).z));
        }
    case u32(3):
        {
            return (textureLoad((hiz_3_0), ((at_2)).xy, ((at_2)).z));
        }
    case u32(4):
        {
            return (textureLoad((hiz_4_0), ((at_2)).xy, ((at_2)).z));
        }
    default :
        {
            return (textureLoad((hiz_5_0), ((at_2)).xy, ((at_2)).z));
        }
    }
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
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_1 : vec2<f32>,
};

@fragment
fn fragmentMain( _S44 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var reflection_0 : vec3<f32>;
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((scene_depth_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var _S45 : i32 = i32(width_0);
    var _S46 : i32 = i32(height_0);
    var extent_4 : vec2<i32> = vec2<i32>(_S45, _S46);
    var _S47 : f32 = f32(width_0);
    var _S48 : f32 = f32(height_0);
    var size_4 : vec2<f32> = vec2<f32>(_S47, _S48);
    var _S49 : vec2<i32> = vec2<i32>(position_1.xy);
    const NOTHING_0 : vec4<f32> = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
    var _S50 : vec3<i32> = vec3<i32>(_S49, i32(0));
    var surface_0 : vec4<f32> = (textureLoad((reflectivity_0), ((_S50)).xy, ((_S50)).z));
    var sharpness_0 : f32 = surface_0.w;
    var depth_3 : f32 = depth_at_0(_S49, extent_4);
    if(depth_3 <= 0.0f)
    {
        var _S51 : pixelOutput_0 = pixelOutput_0( NOTHING_0 );
        return _S51;
    }
    var origin_0 : vec3<f32> = view_position_0(_S49, depth_3, size_4);
    var normal_0 : vec3<f32> = normal_at_0(_S49, origin_0, extent_4, size_4);
    var towards_0 : vec3<f32> = normalize(origin_0);
    var ray_0 : vec3<f32> = reflect(towards_0, normal_0);
    var _S52 : vec4<f32> = vec4<f32>(ray_0, 0.0f);
    var environment_0 : vec3<f32> = probe_environment_0((((vec4<f32>(origin_0, 1.0f)) * (mat4x4<f32>(camera_0.inv_view_0.data_0[i32(0)][i32(0)], camera_0.inv_view_0.data_0[i32(1)][i32(0)], camera_0.inv_view_0.data_0[i32(2)][i32(0)], camera_0.inv_view_0.data_0[i32(3)][i32(0)], camera_0.inv_view_0.data_0[i32(0)][i32(1)], camera_0.inv_view_0.data_0[i32(1)][i32(1)], camera_0.inv_view_0.data_0[i32(2)][i32(1)], camera_0.inv_view_0.data_0[i32(3)][i32(1)], camera_0.inv_view_0.data_0[i32(0)][i32(2)], camera_0.inv_view_0.data_0[i32(1)][i32(2)], camera_0.inv_view_0.data_0[i32(2)][i32(2)], camera_0.inv_view_0.data_0[i32(3)][i32(2)], camera_0.inv_view_0.data_0[i32(0)][i32(3)], camera_0.inv_view_0.data_0[i32(1)][i32(3)], camera_0.inv_view_0.data_0[i32(2)][i32(3)], camera_0.inv_view_0.data_0[i32(3)][i32(3)])))).xyz, normalize((((_S52) * (mat4x4<f32>(camera_0.inv_view_0.data_0[i32(0)][i32(0)], camera_0.inv_view_0.data_0[i32(1)][i32(0)], camera_0.inv_view_0.data_0[i32(2)][i32(0)], camera_0.inv_view_0.data_0[i32(3)][i32(0)], camera_0.inv_view_0.data_0[i32(0)][i32(1)], camera_0.inv_view_0.data_0[i32(1)][i32(1)], camera_0.inv_view_0.data_0[i32(2)][i32(1)], camera_0.inv_view_0.data_0[i32(3)][i32(1)], camera_0.inv_view_0.data_0[i32(0)][i32(2)], camera_0.inv_view_0.data_0[i32(1)][i32(2)], camera_0.inv_view_0.data_0[i32(2)][i32(2)], camera_0.inv_view_0.data_0[i32(3)][i32(2)], camera_0.inv_view_0.data_0[i32(0)][i32(3)], camera_0.inv_view_0.data_0[i32(1)][i32(3)], camera_0.inv_view_0.data_0[i32(2)][i32(3)], camera_0.inv_view_0.data_0[i32(3)][i32(3)])))).xyz));
    var _S53 : vec3<f32> = (vec3<f32>(0) - towards_0);
    var f0_0 : vec3<f32> = surface_0.xyz;
    var grazing_0 : f32 = 1.0f - saturate(dot(normal_0, _S53));
    var grazing2_0 : f32 = grazing_0 * grazing_0;
    var fresnel_0 : vec3<f32> = f0_0 + (vec3<f32>(1.0f, 1.0f, 1.0f) - f0_0) * vec3<f32>((grazing2_0 * grazing2_0 * grazing_0));
    if(sharpness_0 <= 0.0f)
    {
        var _S54 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, 0.0f) );
        return _S54;
    }
    var _S55 : f32 = saturate((1.0f - dot(ray_0, _S53)) / 0.05000000074505806f);
    var _S56 : f32 = origin_0.z;
    var start_0 : vec3<f32> = origin_0 + normal_0 * vec3<f32>((abs(_S56) * 0.00499999988824129f));
    var clip_start_0 : vec4<f32> = (((vec4<f32>(start_0, 1.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var clip_ray_0 : vec4<f32> = (((_S52) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var _S57 : f32 = clip_start_0.w;
    if(_S57 <= 0.0f)
    {
        var _S58 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, sharpness_0) );
        return _S58;
    }
    var _S59 : vec2<f32> = clip_start_0.xy;
    var _S60 : vec2<f32> = vec2<f32>(_S57);
    var at_start_0 : vec2<f32> = pixel_of_0(_S59 / _S60, size_4);
    var _S61 : vec2<f32> = clip_ray_0.xy;
    var _S62 : f32 = clip_ray_0.w;
    var _S63 : vec2<f32> = vec2<f32>(_S62);
    var ndc_rate_0 : vec2<f32> = (_S61 * _S60 - _S59 * _S63) / vec2<f32>((_S57 * _S57));
    var screen_rate_0 : vec2<f32> = vec2<f32>(ndc_rate_0.x * 0.5f * _S47, - ndc_rate_0.y * 0.5f * _S48);
    var rate_0 : f32 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {
        var _S64 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, sharpness_0) );
        return _S64;
    }
    var forward_1 : vec2<f32> = screen_rate_0 / vec2<f32>(rate_0);
    var reach_1 : f32 = 0.75f * min(_S47, _S48);
    var _S65 : f32 = forward_1.x;
    var travel_0 : f32;
    if(_S65 > 0.0f)
    {
        travel_0 = min(reach_1, (_S47 - 1.0f - at_start_0.x) / _S65);
    }
    else
    {
        if(_S65 < 0.0f)
        {
            travel_0 = min(reach_1, - at_start_0.x / _S65);
        }
        else
        {
            travel_0 = reach_1;
        }
    }
    var _S66 : f32 = forward_1.y;
    if(_S66 > 0.0f)
    {
        travel_0 = min(travel_0, (_S48 - 1.0f - at_start_0.y) / _S66);
    }
    else
    {
        if(_S66 < 0.0f)
        {
            travel_0 = min(travel_0, - at_start_0.y / _S66);
        }
    }
    if(_S62 > 0.0f)
    {
        travel_0 = min(travel_0, max(dot(pixel_of_0(_S61 / _S63, size_4) - at_start_0, forward_1) - 1.0f, 0.0f));
    }
    else
    {
        if(_S62 < 0.0f)
        {
            var on_near_0 : vec4<f32> = (((vec4<f32>(0.0f, 0.0f, 1.0f, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
            var clip_near_0 : vec4<f32> = clip_start_0 + clip_ray_0 * vec4<f32>(((- on_near_0.z / on_near_0.w - _S57) / _S62));
            travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / vec2<f32>(clip_near_0.w), size_4) - at_start_0, forward_1), 0.0f));
        }
    }
    var _S67 : f32 = max(travel_0, 0.0f);
    if(_S67 <= 0.00390625f)
    {
        var _S68 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, sharpness_0) );
        return _S68;
    }
    var ndc_end_0 : vec2<f32> = ndc_of_0(at_start_0 + forward_1 * vec2<f32>(_S67), size_4);
    var when_end_0 : f32;
    if((abs(_S65)) >= (abs(_S66)))
    {
        var _S69 : f32 = ndc_end_0.x;
        when_end_0 = (_S69 * _S57 - clip_start_0.x) / (clip_ray_0.x - _S69 * _S62);
    }
    else
    {
        var _S70 : f32 = ndc_end_0.y;
        when_end_0 = (_S70 * _S57 - clip_start_0.y) / (clip_ray_0.y - _S70 * _S62);
    }
    var _S71 : bool;
    if(!(when_end_0 > 0.0f))
    {
        _S71 = true;
    }
    else
    {
        _S71 = !isfinite_0(when_end_0);
    }
    if(_S71)
    {
        var _S72 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, sharpness_0) );
        return _S72;
    }
    var inverse_w_start_0 : f32 = 1.0f / _S57;
    var inverse_w_end_0 : f32 = 1.0f / (_S57 + when_end_0 * _S62);
    var _S73 : f32 = start_0.z;
    var _S74 : f32 = _S73 * inverse_w_start_0;
    var _S75 : f32 = (_S73 + when_end_0 * ray_0.z) * inverse_w_end_0;
    var _S76 : vec3<f32> = environment_0 * fresnel_0;
    var _S77 : u32 = min(camera_0.hiz_0.x, u32(5));
    var _S78 : f32 = _S73 - _S56;
    var at_travel_0 : f32 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S67), _S67);
    var previous_gap_0 : f32 = _S78;
    var entry_z_0 : f32 = _S73;
    var step_0 : u32 = u32(0);
    var level_1 : u32 = u32(0);
    for(;;)
    {
        if(step_0 < u32(96))
        {
        }
        else
        {
            reflection_0 = _S76;
            break;
        }
        var cell_1 : f32 = f32((u32(1) << (level_1)));
        var at_3 : vec2<f32> = at_start_0 + forward_1 * vec2<f32>(at_travel_0);
        var _S79 : f32 = min(at_travel_0 + cell_exit_0(at_3, forward_1, cell_1, _S67), _S67);
        var exit_at_0 : vec2<f32> = at_start_0 + forward_1 * vec2<f32>(_S79);
        var along_0 : f32 = _S79 / _S67;
        var exit_z_0 : f32 = mix(_S74, _S75, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);
        var cell_depth_0 : f32 = hiz_at_0(level_1, vec2<i32>(floor(at_3 / vec2<f32>(cell_1))), vec2<i32>((_S45 >> (level_1)), (_S46 >> (level_1))));
        var gap_0 : f32;
        if(cell_depth_0 <= 0.0f)
        {
            gap_0 = 1.0f;
        }
        else
        {
            gap_0 = exit_z_0 - view_z_of_0(cell_depth_0);
        }
        var _S80 : bool = !(gap_0 > 0.0f);
        if(_S80)
        {
            _S71 = level_1 > u32(0);
        }
        else
        {
            _S71 = false;
        }
        if(_S71)
        {
            level_1 = level_1 - u32(1);
            step_0 = step_0 + u32(1);
            continue;
        }
        var _S81 : bool;
        if(_S80)
        {
            _S81 = previous_gap_0 > 0.0f;
        }
        else
        {
            _S81 = false;
        }
        if(_S81)
        {
            var behind_0 : f32 = - gap_0;
            var thickness_0 : f32 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {
                var hit_at_0 : vec2<f32> = mix(at_3, exit_at_0, vec2<f32>((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))));
                var hit_ndc_0 : vec2<f32> = ndc_of_0(hit_at_0, size_4);
                var confidence_0 : f32 = sharpness_0 * _S55 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S79 / reach_1) / 0.25f) * saturate(1.0f - behind_0 / thickness_0);
                var _S82 : vec3<i32> = vec3<i32>(clamp(vec2<i32>(hit_at_0), vec2<i32>(i32(0), i32(0)), extent_4 - vec2<i32>(i32(1), i32(1))), i32(0));
                reflection_0 = (textureLoad((scene_color_0), ((_S82)).xy, ((_S82)).z)).xyz * fresnel_0 * vec3<f32>(confidence_0) + _S76 * vec3<f32>((1.0f - confidence_0));
                break;
            }
        }
        if(_S79 >= _S67)
        {
            reflection_0 = _S76;
            break;
        }
        var _S83 : u32 = min(level_1 + u32(1), _S77);
        at_travel_0 = _S79;
        previous_gap_0 = gap_0;
        entry_z_0 = exit_z_0;
        level_1 = _S83;
        step_0 = step_0 + u32(1);
    }
    var _S84 : pixelOutput_0 = pixelOutput_0( vec4<f32>(reflection_0, sharpness_0) );
    return _S84;
}

