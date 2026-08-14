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
};

@binding(0) @group(0) var<uniform> camera_0 : SsrParams_std140_0;
struct GpuProbe_std430_0
{
    @align(16) sh_r_0 : vec4<f32>,
    @align(16) sh_g_0 : vec4<f32>,
    @align(16) sh_b_0 : vec4<f32>,
};

@binding(4) @group(0) var<storage, read> probes_0 : array<GpuProbe_std430_0>;

@binding(2) @group(0) var scene_color_0 : texture_2d<f32>;

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

struct GpuProbe_0
{
     sh_r_0 : vec4<f32>,
     sh_g_0 : vec4<f32>,
     sh_b_0 : vec4<f32>,
};

fn probe_environment_0( world_position_0 : vec3<f32>,  direction_0 : vec3<f32>) -> vec3<f32>
{
    var _S8 : vec3<f32> = vec3<f32>(1.0f);
    const _S9 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var last_0 : vec3<f32> = max(vec3<f32>(camera_0.probe_counts_0.xyz) - _S8, _S9);
    var grid_0 : vec3<f32> = clamp((world_position_0 - camera_0.probe_origin_0.xyz) * camera_0.probe_inv_spacing_0.xyz, _S9, last_0);
    var base_0 : vec3<f32> = floor(grid_0);
    var f_0 : vec3<f32> = grid_0 - base_0;
    var _S10 : vec3<u32> = vec3<u32>(base_0);
    var _S11 : vec3<u32> = vec3<u32>(min(base_0 + _S8, last_0));
    var total_0 : u32 = max(camera_0.probe_counts_0.w, u32(1)) - u32(1);
    var _S12 : u32 = _S10.z;
    var _S13 : u32 = _S10.y;
    var _S14 : u32 = _S10.x;
    var _S15 : u32 = _S11.x;
    var _S16 : u32 = _S11.y;
    var _S17 : u32 = _S11.z;
    var x00_0 : GpuProbe_std430_0 = probes_0[min((_S12 * camera_0.probe_counts_0.y + _S13) * camera_0.probe_counts_0.x + _S14, total_0)];
    var x10_0 : GpuProbe_std430_0 = probes_0[min((_S12 * camera_0.probe_counts_0.y + _S16) * camera_0.probe_counts_0.x + _S14, total_0)];
    var x01_0 : GpuProbe_std430_0 = probes_0[min((_S17 * camera_0.probe_counts_0.y + _S13) * camera_0.probe_counts_0.x + _S14, total_0)];
    var x11_0 : GpuProbe_std430_0 = probes_0[min((_S17 * camera_0.probe_counts_0.y + _S16) * camera_0.probe_counts_0.x + _S14, total_0)];
    var y00_0 : GpuProbe_std430_0 = probes_0[min((_S12 * camera_0.probe_counts_0.y + _S13) * camera_0.probe_counts_0.x + _S15, total_0)];
    var y10_0 : GpuProbe_std430_0 = probes_0[min((_S12 * camera_0.probe_counts_0.y + _S16) * camera_0.probe_counts_0.x + _S15, total_0)];
    var y01_0 : GpuProbe_std430_0 = probes_0[min((_S17 * camera_0.probe_counts_0.y + _S13) * camera_0.probe_counts_0.x + _S15, total_0)];
    var y11_0 : GpuProbe_std430_0 = probes_0[min((_S17 * camera_0.probe_counts_0.y + _S16) * camera_0.probe_counts_0.x + _S15, total_0)];
    var z0_0 : GpuProbe_0;
    var _S18 : vec4<f32> = vec4<f32>(f_0.x);
    var _S19 : vec4<f32> = vec4<f32>(f_0.y);
    var _S20 : vec4<f32> = mix(mix(x00_0.sh_r_0, y00_0.sh_r_0, _S18), mix(x10_0.sh_r_0, y10_0.sh_r_0, _S18), _S19);
    z0_0.sh_r_0 = _S20;
    var _S21 : vec4<f32> = mix(mix(x00_0.sh_g_0, y00_0.sh_g_0, _S18), mix(x10_0.sh_g_0, y10_0.sh_g_0, _S18), _S19);
    z0_0.sh_g_0 = _S21;
    var _S22 : vec4<f32> = mix(mix(x00_0.sh_b_0, y00_0.sh_b_0, _S18), mix(x10_0.sh_b_0, y10_0.sh_b_0, _S18), _S19);
    z0_0.sh_b_0 = _S22;
    var z1_0 : GpuProbe_0;
    var _S23 : vec4<f32> = mix(mix(x01_0.sh_r_0, y01_0.sh_r_0, _S18), mix(x11_0.sh_r_0, y11_0.sh_r_0, _S18), _S19);
    z1_0.sh_r_0 = _S23;
    var _S24 : vec4<f32> = mix(mix(x01_0.sh_g_0, y01_0.sh_g_0, _S18), mix(x11_0.sh_g_0, y11_0.sh_g_0, _S18), _S19);
    z1_0.sh_g_0 = _S24;
    var _S25 : vec4<f32> = mix(mix(x01_0.sh_b_0, y01_0.sh_b_0, _S18), mix(x11_0.sh_b_0, y11_0.sh_b_0, _S18), _S19);
    z1_0.sh_b_0 = _S25;
    var cell_0 : GpuProbe_0;
    var _S26 : vec4<f32> = vec4<f32>(f_0.z);
    var _S27 : vec4<f32> = mix(_S20, _S23, _S26);
    cell_0.sh_r_0 = _S27;
    var _S28 : vec4<f32> = mix(_S21, _S24, _S26);
    cell_0.sh_g_0 = _S28;
    var _S29 : vec4<f32> = mix(_S22, _S25, _S26);
    cell_0.sh_b_0 = _S29;
    var _S30 : vec3<f32> = vec3<f32>(2.09439516067504883f);
    return max(vec3<f32>(dot(_S27.xyz / _S30, direction_0) + _S27.w / 3.14159274101257324f, dot(_S28.xyz / _S30, direction_0) + _S28.w / 3.14159274101257324f, dot(_S29.xyz / _S30, direction_0) + _S29.w / 3.14159274101257324f), _S9);
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
fn fragmentMain( _S31 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var reflection_0 : vec3<f32>;
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((scene_depth_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var extent_3 : vec2<i32> = vec2<i32>(i32(width_0), i32(height_0));
    var _S32 : f32 = f32(width_0);
    var _S33 : f32 = f32(height_0);
    var size_3 : vec2<f32> = vec2<f32>(_S32, _S33);
    var _S34 : vec2<i32> = vec2<i32>(position_1.xy);
    const NOTHING_0 : vec4<f32> = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
    var _S35 : vec3<i32> = vec3<i32>(_S34, i32(0));
    var surface_0 : vec4<f32> = (textureLoad((reflectivity_0), ((_S35)).xy, ((_S35)).z));
    var sharpness_0 : f32 = surface_0.w;
    var depth_2 : f32 = depth_at_0(_S34, extent_3);
    if(depth_2 <= 0.0f)
    {
        var _S36 : pixelOutput_0 = pixelOutput_0( NOTHING_0 );
        return _S36;
    }
    var origin_0 : vec3<f32> = view_position_0(_S34, depth_2, size_3);
    var normal_0 : vec3<f32> = normal_at_0(_S34, origin_0, extent_3, size_3);
    var towards_0 : vec3<f32> = normalize(origin_0);
    var ray_0 : vec3<f32> = reflect(towards_0, normal_0);
    var _S37 : vec4<f32> = vec4<f32>(ray_0, 0.0f);
    var environment_0 : vec3<f32> = probe_environment_0((((vec4<f32>(origin_0, 1.0f)) * (mat4x4<f32>(camera_0.inv_view_0.data_0[i32(0)][i32(0)], camera_0.inv_view_0.data_0[i32(1)][i32(0)], camera_0.inv_view_0.data_0[i32(2)][i32(0)], camera_0.inv_view_0.data_0[i32(3)][i32(0)], camera_0.inv_view_0.data_0[i32(0)][i32(1)], camera_0.inv_view_0.data_0[i32(1)][i32(1)], camera_0.inv_view_0.data_0[i32(2)][i32(1)], camera_0.inv_view_0.data_0[i32(3)][i32(1)], camera_0.inv_view_0.data_0[i32(0)][i32(2)], camera_0.inv_view_0.data_0[i32(1)][i32(2)], camera_0.inv_view_0.data_0[i32(2)][i32(2)], camera_0.inv_view_0.data_0[i32(3)][i32(2)], camera_0.inv_view_0.data_0[i32(0)][i32(3)], camera_0.inv_view_0.data_0[i32(1)][i32(3)], camera_0.inv_view_0.data_0[i32(2)][i32(3)], camera_0.inv_view_0.data_0[i32(3)][i32(3)])))).xyz, normalize((((_S37) * (mat4x4<f32>(camera_0.inv_view_0.data_0[i32(0)][i32(0)], camera_0.inv_view_0.data_0[i32(1)][i32(0)], camera_0.inv_view_0.data_0[i32(2)][i32(0)], camera_0.inv_view_0.data_0[i32(3)][i32(0)], camera_0.inv_view_0.data_0[i32(0)][i32(1)], camera_0.inv_view_0.data_0[i32(1)][i32(1)], camera_0.inv_view_0.data_0[i32(2)][i32(1)], camera_0.inv_view_0.data_0[i32(3)][i32(1)], camera_0.inv_view_0.data_0[i32(0)][i32(2)], camera_0.inv_view_0.data_0[i32(1)][i32(2)], camera_0.inv_view_0.data_0[i32(2)][i32(2)], camera_0.inv_view_0.data_0[i32(3)][i32(2)], camera_0.inv_view_0.data_0[i32(0)][i32(3)], camera_0.inv_view_0.data_0[i32(1)][i32(3)], camera_0.inv_view_0.data_0[i32(2)][i32(3)], camera_0.inv_view_0.data_0[i32(3)][i32(3)])))).xyz));
    var _S38 : vec3<f32> = (vec3<f32>(0) - towards_0);
    var f0_0 : vec3<f32> = surface_0.xyz;
    var grazing_0 : f32 = 1.0f - saturate(dot(normal_0, _S38));
    var grazing2_0 : f32 = grazing_0 * grazing_0;
    var fresnel_0 : vec3<f32> = f0_0 + (vec3<f32>(1.0f, 1.0f, 1.0f) - f0_0) * vec3<f32>((grazing2_0 * grazing2_0 * grazing_0));
    if(sharpness_0 <= 0.0f)
    {
        var _S39 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, 0.0f) );
        return _S39;
    }
    var _S40 : f32 = saturate((1.0f - dot(ray_0, _S38)) / 0.05000000074505806f);
    var _S41 : f32 = origin_0.z;
    var start_0 : vec3<f32> = origin_0 + normal_0 * vec3<f32>((abs(_S41) * 0.00499999988824129f));
    var clip_start_0 : vec4<f32> = (((vec4<f32>(start_0, 1.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var clip_ray_0 : vec4<f32> = (((_S37) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var _S42 : f32 = clip_start_0.w;
    if(_S42 <= 0.0f)
    {
        var _S43 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, sharpness_0) );
        return _S43;
    }
    var _S44 : vec2<f32> = clip_start_0.xy;
    var _S45 : vec2<f32> = vec2<f32>(_S42);
    var at_start_0 : vec2<f32> = pixel_of_0(_S44 / _S45, size_3);
    var _S46 : vec2<f32> = clip_ray_0.xy;
    var _S47 : f32 = clip_ray_0.w;
    var _S48 : vec2<f32> = vec2<f32>(_S47);
    var ndc_rate_0 : vec2<f32> = (_S46 * _S45 - _S44 * _S48) / vec2<f32>((_S42 * _S42));
    var screen_rate_0 : vec2<f32> = vec2<f32>(ndc_rate_0.x * 0.5f * _S32, - ndc_rate_0.y * 0.5f * _S33);
    var rate_0 : f32 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {
        var _S49 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, sharpness_0) );
        return _S49;
    }
    var forward_0 : vec2<f32> = screen_rate_0 / vec2<f32>(rate_0);
    var stride_0 : f32 = 0.75f * min(_S32, _S33) / 96.0f;
    var travel_0 : f32 = 96.0f * stride_0;
    var _S50 : f32 = forward_0.x;
    var travel_1 : f32;
    if(_S50 > 0.0f)
    {
        travel_1 = min(travel_0, (_S32 - 1.0f - at_start_0.x) / _S50);
    }
    else
    {
        if(_S50 < 0.0f)
        {
            travel_1 = min(travel_0, - at_start_0.x / _S50);
        }
        else
        {
            travel_1 = travel_0;
        }
    }
    var _S51 : f32 = forward_0.y;
    if(_S51 > 0.0f)
    {
        travel_1 = min(travel_1, (_S33 - 1.0f - at_start_0.y) / _S51);
    }
    else
    {
        if(_S51 < 0.0f)
        {
            travel_1 = min(travel_1, - at_start_0.y / _S51);
        }
    }
    if(_S47 > 0.0f)
    {
        travel_1 = min(travel_1, max(dot(pixel_of_0(_S46 / _S48, size_3) - at_start_0, forward_0), 0.0f));
    }
    else
    {
        if(_S47 < 0.0f)
        {
            var on_near_0 : vec4<f32> = (((vec4<f32>(0.0f, 0.0f, 1.0f, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
            var clip_near_0 : vec4<f32> = clip_start_0 + clip_ray_0 * vec4<f32>(((- on_near_0.z / on_near_0.w - _S42) / _S47));
            travel_1 = min(travel_1, max(dot(pixel_of_0(clip_near_0.xy / vec2<f32>(clip_near_0.w), size_3) - at_start_0, forward_0), 0.0f));
        }
    }
    var steps_0 : u32 = u32(max(travel_1, 0.0f) / stride_0);
    if(steps_0 == u32(0))
    {
        var _S52 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, sharpness_0) );
        return _S52;
    }
    var _S53 : f32 = f32(steps_0);
    var travel_2 : f32 = _S53 * stride_0;
    var ndc_end_0 : vec2<f32> = ndc_of_0(at_start_0 + forward_0 * vec2<f32>(travel_2), size_3);
    var when_end_0 : f32;
    if((abs(_S50)) >= (abs(_S51)))
    {
        var _S54 : f32 = ndc_end_0.x;
        when_end_0 = (_S54 * _S42 - clip_start_0.x) / (clip_ray_0.x - _S54 * _S47);
    }
    else
    {
        var _S55 : f32 = ndc_end_0.y;
        when_end_0 = (_S55 * _S42 - clip_start_0.y) / (clip_ray_0.y - _S55 * _S47);
    }
    if(!(when_end_0 > 0.0f))
    {
        var _S56 : pixelOutput_0 = pixelOutput_0( vec4<f32>(environment_0 * fresnel_0, sharpness_0) );
        return _S56;
    }
    var inverse_w_start_0 : f32 = 1.0f / _S42;
    var inverse_w_end_0 : f32 = 1.0f / (_S42 + when_end_0 * _S47);
    var _S57 : f32 = start_0.z;
    var _S58 : f32 = _S57 * inverse_w_start_0;
    var _S59 : f32 = (_S57 + when_end_0 * ray_0.z) * inverse_w_end_0;
    var _S60 : vec3<f32> = environment_0 * fresnel_0;
    var previous_gap_0 : f32 = _S57 - _S41;
    var previous_z_0 : f32 = _S57;
    var previous_at_0 : vec2<f32> = at_start_0;
    var step_0 : u32 = u32(1);
    for(;;)
    {
        if(step_0 <= steps_0)
        {
        }
        else
        {
            reflection_0 = _S60;
            break;
        }
        var _S61 : f32 = f32(step_0);
        var along_0 : f32 = _S61 / _S53;
        var at_1 : vec2<f32> = at_start_0 + forward_0 * vec2<f32>((travel_2 * along_0));
        var _S62 : vec2<i32> = vec2<i32>(at_1);
        var ray_z_0 : f32 = mix(_S58, _S59, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);
        var tapped_0 : f32 = depth_at_0(_S62, extent_3);
        var gap_0 : f32;
        if(tapped_0 > 0.0f)
        {
            gap_0 = ray_z_0 - view_position_0(_S62, tapped_0, size_3).z;
        }
        else
        {
            gap_0 = 1.0f;
        }
        var _S63 : bool;
        if(previous_gap_0 > 0.0f)
        {
            _S63 = gap_0 < 0.0f;
        }
        else
        {
            _S63 = false;
        }
        if(_S63)
        {
            var behind_0 : f32 = - gap_0;
            var thickness_0 : f32 = thickness_at_0(abs(ray_z_0 - previous_z_0), ray_z_0);
            if(behind_0 <= thickness_0)
            {
                var hit_at_0 : vec2<f32> = mix(previous_at_0, at_1, vec2<f32>((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))));
                var hit_ndc_0 : vec2<f32> = ndc_of_0(hit_at_0, size_3);
                var confidence_0 : f32 = sharpness_0 * _S40 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S61 / 96.0f) / 0.25f) * saturate(1.0f - behind_0 / thickness_0);
                var _S64 : vec3<i32> = vec3<i32>(clamp(vec2<i32>(hit_at_0), vec2<i32>(i32(0), i32(0)), extent_3 - vec2<i32>(i32(1), i32(1))), i32(0));
                reflection_0 = (textureLoad((scene_color_0), ((_S64)).xy, ((_S64)).z)).xyz * fresnel_0 * vec3<f32>(confidence_0) + _S60 * vec3<f32>((1.0f - confidence_0));
                break;
            }
        }
        var step_1 : u32 = step_0 + u32(1);
        previous_gap_0 = gap_0;
        previous_z_0 = ray_z_0;
        previous_at_0 = at_1;
        step_0 = step_1;
    }
    var _S65 : pixelOutput_0 = pixelOutput_0( vec4<f32>(reflection_0, sharpness_0) );
    return _S65;
}

