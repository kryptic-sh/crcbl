struct GatherParams_std140_0
{
    @align(16) sun_color_0 : vec4<f32>,
    @align(16) texel_area_0 : f32,
    @align(4) rsm_side_0 : u32,
    @align(8) probes_0 : u32,
    @align(4) reserved_0 : u32,
};

@binding(0) @group(0) var<uniform> params_0 : GatherParams_std140_0;
@binding(1) @group(0) var<storage, read> probe_positions_0 : array<vec4<f32>>;

@binding(5) @group(0) var rsm_world_0 : texture_2d<f32>;

@binding(4) @group(0) var rsm_normal_0 : texture_2d<f32>;

@binding(2) @group(0) var probe_visibility_0 : texture_2d_array<f32>;

@binding(3) @group(0) var rsm_albedo_0 : texture_2d<f32>;

@binding(6) @group(0) var<storage, read_write> probes_1 : array<vec4<f32>>;

fn sign_not_zero_0( value_0 : f32) -> f32
{
    var _S1 : f32;
    if(value_0 >= 0.0f)
    {
        _S1 = 1.0f;
    }
    else
    {
        _S1 = -1.0f;
    }
    return _S1;
}

fn oct_encode_0( direction_0 : vec3<f32>) -> vec2<f32>
{
    var _S2 : f32 = direction_0.y;
    var p_0 : vec2<f32> = direction_0.xz / vec2<f32>(max(abs(direction_0.x) + abs(_S2) + abs(direction_0.z), 9.99999968265522539e-21f));
    var p_1 : vec2<f32>;
    if(_S2 < 0.0f)
    {
        var _S3 : f32 = p_0.y;
        var _S4 : f32 = p_0.x;
        p_1 = vec2<f32>((1.0f - abs(_S3)) * sign_not_zero_0(_S4), (1.0f - abs(_S4)) * sign_not_zero_0(_S3));
    }
    else
    {
        p_1 = p_0;
    }
    return p_1;
}

fn probe_moments_0( index_0 : u32,  direction_1 : vec3<f32>) -> vec2<f32>
{
    var width_0 : u32;
    var height_0 : u32;
    var layers_0 : u32;
    {var dim = textureDimensions((probe_visibility_0));((width_0)) = dim.x;((height_0)) = dim.y;((layers_0)) = textureNumLayers((probe_visibility_0));};
    var _S5 : vec2<f32> = vec2<f32>(0.5f);
    var _S6 : vec2<f32> = vec2<f32>(1.0f);
    var scaled_0 : vec2<f32> = (oct_encode_0(direction_1) * _S5 + _S5) * vec2<f32>(16.0f) + _S6 - _S5;
    var _S7 : vec2<f32> = vec2<f32>(f32(width_0), f32(height_0)) - _S6;
    var low_0 : vec2<f32> = clamp(floor(scaled_0), vec2<f32>(0.0f, 0.0f), _S7);
    var high_0 : vec2<f32> = min(low_0 + _S6, _S7);
    var weight_0 : vec2<f32> = clamp(scaled_0 - low_0, vec2<f32>(0.0f), vec2<f32>(1.0f));
    var layer_0 : i32 = i32(min(index_0, max(layers_0, u32(1)) - u32(1)));
    var _S8 : i32 = i32(low_0.x);
    var _S9 : i32 = i32(low_0.y);
    var _S10 : vec4<i32> = vec4<i32>(_S8, _S9, layer_0, i32(0));
    var _S11 : i32 = i32(high_0.x);
    var _S12 : vec4<i32> = vec4<i32>(_S11, _S9, layer_0, i32(0));
    var _S13 : i32 = i32(high_0.y);
    var _S14 : vec4<i32> = vec4<i32>(_S8, _S13, layer_0, i32(0));
    var _S15 : vec4<i32> = vec4<i32>(_S11, _S13, layer_0, i32(0));
    var _S16 : vec2<f32> = vec2<f32>(weight_0.x);
    return mix(mix((textureLoad((probe_visibility_0), ((_S10)).xy, i32(((_S10)).z), ((_S10)).w)).xy, (textureLoad((probe_visibility_0), ((_S12)).xy, i32(((_S12)).z), ((_S12)).w)).xy, _S16), mix((textureLoad((probe_visibility_0), ((_S14)).xy, i32(((_S14)).z), ((_S14)).w)).xy, (textureLoad((probe_visibility_0), ((_S15)).xy, i32(((_S15)).z), ((_S15)).w)).xy, _S16), vec2<f32>(weight_0.y));
}

fn probe_chebyshev_0( index_1 : u32,  probe_position_0 : vec3<f32>,  world_position_0 : vec3<f32>,  normal_0 : vec3<f32>) -> f32
{
    var to_probe_0 : vec3<f32> = probe_position_0 - (world_position_0 + normal_0 * vec3<f32>(0.05000000074505806f));
    var to_surface_0 : f32 = length(to_probe_0);
    var moments_0 : vec2<f32> = probe_moments_0(index_1, (vec3<f32>(0) - to_probe_0));
    var _S17 : f32 = moments_0.x;
    var _S18 : f32 = max(moments_0.y - _S17 * _S17, 0.0f);
    var behind_0 : f32 = to_surface_0 - _S17;
    var bound_0 : f32 = _S18 / (_S18 + behind_0 * behind_0);
    var _S19 : f32;
    if(to_surface_0 <= _S17)
    {
        _S19 = 1.0f;
    }
    else
    {
        _S19 = bound_0 * bound_0 * bound_0;
    }
    return _S19;
}

struct Bands_0
{
     r_0 : vec4<f32>,
     g_0 : vec4<f32>,
     b_0 : vec4<f32>,
};

fn accumulate_0( bands_0 : ptr<function, Bands_0>,  direction_2 : vec3<f32>,  radiance_0 : vec3<f32>,  solid_angle_0 : f32)
{
    var basis_0 : vec4<f32> = vec4<f32>(direction_2 * vec3<f32>((solid_angle_0 * 0.5f)), solid_angle_0 * 0.25f);
    (*bands_0).r_0 = (*bands_0).r_0 + basis_0 * vec4<f32>(radiance_0.x);
    (*bands_0).g_0 = (*bands_0).g_0 + basis_0 * vec4<f32>(radiance_0.y);
    (*bands_0).b_0 = (*bands_0).b_0 + basis_0 * vec4<f32>(radiance_0.z);
    return;
}

var<workgroup> tile_0 : array<Bands_0, i32(64)>;

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(workgroup_id) group_0 : vec3<u32>, @builtin(local_invocation_id) thread_0 : vec3<u32>)
{
    var probe_0 : u32 = group_0.x;
    var lane_0 : u32 = thread_0.x;
    var bands_1 : Bands_0;
    const _S20 : vec4<f32> = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
    bands_1.r_0 = _S20;
    bands_1.g_0 = _S20;
    bands_1.b_0 = _S20;
    var stride_0 : u32;
    if(probe_0 < (params_0.probes_0))
    {
        var _S21 : vec3<f32> = probe_positions_0[probe_0].xyz;
        var _S22 : u32 = max(params_0.rsm_side_0, u32(1));
        var _S23 : u32 = _S22 * _S22;
        stride_0 = lane_0;
        for(;;)
        {
            if(stride_0 < _S23)
            {
            }
            else
            {
                break;
            }
            var row_0 : u32 = stride_0 / _S22;
            var at_0 : vec3<i32> = vec3<i32>(i32(stride_0 - row_0 * _S22), i32(row_0), i32(0));
            var world_0 : vec4<f32> = (textureLoad((rsm_world_0), ((at_0)).xy, ((at_0)).z));
            if((world_0.w) <= 0.0f)
            {
                stride_0 = stride_0 + u32(64);
                continue;
            }
            var sample_position_0 : vec3<f32> = world_0.xyz;
            var sample_normal_0 : vec3<f32> = normalize((textureLoad((rsm_normal_0), ((at_0)).xy, ((at_0)).z)).xyz * vec3<f32>(2.0f) - vec3<f32>(1.0f));
            var offset_0 : vec3<f32> = sample_position_0 - _S21;
            var distance_squared_0 : f32 = dot(offset_0, offset_0);
            if(distance_squared_0 <= 9.999999960041972e-13f)
            {
                stride_0 = stride_0 + u32(64);
                continue;
            }
            var direction_3 : vec3<f32> = offset_0 / vec3<f32>(sqrt(distance_squared_0));
            var facing_0 : f32 = dot(sample_normal_0, (vec3<f32>(0) - direction_3));
            if(facing_0 <= 0.0f)
            {
                stride_0 = stride_0 + u32(64);
                continue;
            }
            var visibility_0 : f32 = probe_chebyshev_0(probe_0, _S21, sample_position_0, sample_normal_0);
            if(visibility_0 <= 0.0f)
            {
                stride_0 = stride_0 + u32(64);
                continue;
            }
            accumulate_0(&(bands_1), direction_3, (textureLoad((rsm_albedo_0), ((at_0)).xy, ((at_0)).z)).xyz * params_0.sun_color_0.xyz * vec3<f32>(0.31830987334251404f) * vec3<f32>(visibility_0), min(params_0.texel_area_0 * facing_0 / distance_squared_0, 6.28318548202514648f));
            stride_0 = stride_0 + u32(64);
        }
    }
    tile_0[lane_0] = bands_1;
    workgroupBarrier();
    stride_0 = u32(32);
    for(;;)
    {
        if(stride_0 > u32(0))
        {
        }
        else
        {
            break;
        }
        if(lane_0 < stride_0)
        {
            tile_0[lane_0].r_0 = tile_0[lane_0].r_0 + tile_0[lane_0 + stride_0].r_0;
            tile_0[lane_0].g_0 = tile_0[lane_0].g_0 + tile_0[lane_0 + stride_0].g_0;
            tile_0[lane_0].b_0 = tile_0[lane_0].b_0 + tile_0[lane_0 + stride_0].b_0;
        }
        workgroupBarrier();
        stride_0 = (stride_0 >> (u32(1)));
    }
    var _S24 : bool;
    if(lane_0 == u32(0))
    {
        _S24 = probe_0 < (params_0.probes_0);
    }
    else
    {
        _S24 = false;
    }
    if(_S24)
    {
        var _S25 : u32 = probe_0 * u32(3);
        probes_1[_S25] = tile_0[i32(0)].r_0;
        probes_1[_S25 + u32(1)] = tile_0[i32(0)].g_0;
        probes_1[_S25 + u32(2)] = tile_0[i32(0)].b_0;
    }
    return;
}

