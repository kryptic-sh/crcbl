struct GatherParams_std140_0
{
    @align(16) sun_color_0 : vec4<f32>,
    @align(16) texel_area_0 : f32,
    @align(4) rsm_side_0 : u32,
    @align(8) probes_0 : u32,
    @align(4) producers_0 : u32,
};

@binding(0) @group(0) var<uniform> params_0 : GatherParams_std140_0;
@binding(1) @group(0) var<storage, read> probe_positions_0 : array<vec4<f32>>;

@binding(5) @group(0) var rsm_world_0 : texture_2d<f32>;

@binding(4) @group(0) var rsm_normal_0 : texture_2d<f32>;

@binding(3) @group(0) var rsm_albedo_0 : texture_2d<f32>;

@binding(2) @group(0) var probe_visibility_0 : texture_2d_array<f32>;

struct PunctualProducer_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
    @align(16) axis_0 : vec4<f32>,
    @align(16) tile_0 : vec4<u32>,
};

@binding(10) @group(0) var<storage, read> producers_1 : array<PunctualProducer_std430_0>;

@binding(9) @group(0) var punctual_world_0 : texture_2d<f32>;

@binding(8) @group(0) var punctual_normal_0 : texture_2d<f32>;

@binding(7) @group(0) var punctual_albedo_0 : texture_2d<f32>;

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

fn gather_patch_0( bands_1 : ptr<function, Bands_0>,  probe_0 : u32,  probe_position_1 : vec3<f32>,  sample_position_0 : vec3<f32>,  sample_normal_0 : vec3<f32>,  radiance_1 : vec3<f32>,  patch_area_0 : f32)
{
    var offset_0 : vec3<f32> = sample_position_0 - probe_position_1;
    var distance_squared_0 : f32 = dot(offset_0, offset_0);
    if(distance_squared_0 <= 9.999999960041972e-13f)
    {
        return;
    }
    var direction_3 : vec3<f32> = offset_0 / vec3<f32>(sqrt(distance_squared_0));
    var facing_0 : f32 = dot(sample_normal_0, (vec3<f32>(0) - direction_3));
    if(facing_0 <= 0.0f)
    {
        return;
    }
    var visibility_0 : f32 = probe_chebyshev_0(probe_0, probe_position_1, sample_position_0, sample_normal_0);
    if(visibility_0 <= 0.0f)
    {
        return;
    }
    accumulate_0(&((*bands_1)), direction_3, radiance_1 * vec3<f32>(visibility_0), min(patch_area_0 * facing_0 / distance_squared_0, 6.28318548202514648f));
    return;
}

fn producer_tangent_0( light_0 : ptr<function, PunctualProducer_std430_0>) -> f32
{
    if(((*light_0).tile_0.w) != u32(2))
    {
        return 1.0f;
    }
    var _S20 : f32 = max((*light_0).color_0.w, 0.00100000004749745f);
    return sqrt(max(1.0f - _S20 * _S20, 0.0f)) / _S20;
}

fn spot_cone_0( to_light_0 : vec3<f32>,  axis_1 : vec3<f32>,  cos_outer_0 : f32,  cos_inner_0 : f32) -> f32
{
    return saturate((dot((vec3<f32>(0) - to_light_0), normalize(axis_1)) - cos_outer_0) / max(cos_inner_0 - cos_outer_0, 0.00009999999747379f));
}

fn range_window_0( distance_0 : f32,  radius_0 : f32) -> f32
{
    var ratio_0 : f32 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    var window_0 : f32 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}

fn punctual_falloff_0( distance_1 : f32,  radius_1 : f32) -> f32
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}

var<workgroup> tile_1 : array<Bands_0, i32(64)>;

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(workgroup_id) group_0 : vec3<u32>, @builtin(local_invocation_id) thread_0 : vec3<u32>)
{
    var probe_1 : u32 = group_0.x;
    var lane_0 : u32 = thread_0.x;
    var bands_2 : Bands_0;
    const _S21 : vec4<f32> = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
    bands_2.r_0 = _S21;
    bands_2.g_0 = _S21;
    bands_2.b_0 = _S21;
    var stride_0 : u32;
    if(probe_1 < (params_0.probes_0))
    {
        var _S22 : vec3<f32> = probe_positions_0[probe_1].xyz;
        var _S23 : u32 = max(params_0.rsm_side_0, u32(1));
        var _S24 : u32 = _S23 * _S23;
        stride_0 = lane_0;
        for(;;)
        {
            if(stride_0 < _S24)
            {
            }
            else
            {
                break;
            }
            var row_0 : u32 = stride_0 / _S23;
            var at_0 : vec3<i32> = vec3<i32>(i32(stride_0 - row_0 * _S23), i32(row_0), i32(0));
            var world_0 : vec4<f32> = (textureLoad((rsm_world_0), ((at_0)).xy, ((at_0)).z));
            if((world_0.w) <= 0.0f)
            {
                stride_0 = stride_0 + u32(64);
                continue;
            }
            gather_patch_0(&(bands_2), probe_1, _S22, world_0.xyz, normalize((textureLoad((rsm_normal_0), ((at_0)).xy, ((at_0)).z)).xyz * vec3<f32>(2.0f) - vec3<f32>(1.0f)), (textureLoad((rsm_albedo_0), ((at_0)).xy, ((at_0)).z)).xyz * params_0.sun_color_0.xyz * vec3<f32>(0.31830987334251404f), params_0.texel_area_0);
            stride_0 = stride_0 + u32(64);
        }
        var producer_0 : u32 = u32(0);
        for(;;)
        {
            if(producer_0 < (params_0.producers_0))
            {
            }
            else
            {
                break;
            }
            var _S25 : PunctualProducer_std430_0 = producers_1[producer_0];
            var _S26 : vec4<u32> = _S25.tile_0;
            var _S27 : u32 = max(_S25.tile_0.z, u32(1));
            var _S28 : u32 = _S27 * _S27;
            var _S29 : f32 = producer_tangent_0(&(_S25));
            var _S30 : f32 = f32(_S27);
            var _S31 : f32 = 2.0f * _S29 / _S30;
            stride_0 = lane_0;
            for(;;)
            {
                if(stride_0 < _S28)
                {
                }
                else
                {
                    break;
                }
                var row_1 : u32 = stride_0 / _S27;
                var column_0 : u32 = stride_0 - row_1 * _S27;
                var at_1 : vec3<i32> = vec3<i32>(i32(_S26.x + column_0), i32(_S26.y + row_1), i32(0));
                var world_1 : vec4<f32> = (textureLoad((punctual_world_0), ((at_1)).xy, ((at_1)).z));
                if((world_1.w) <= 0.0f)
                {
                    stride_0 = stride_0 + u32(64);
                    continue;
                }
                var sample_normal_1 : vec3<f32> = normalize((textureLoad((punctual_normal_0), ((at_1)).xy, ((at_1)).z)).xyz * vec3<f32>(2.0f) - vec3<f32>(1.0f));
                var _S32 : vec4<f32> = _S25.position_0;
                var _S33 : vec3<f32> = world_1.xyz;
                var to_light_1 : vec3<f32> = _S25.position_0.xyz - _S33;
                var to_light_distance_0 : f32 = length(to_light_1);
                if(to_light_distance_0 <= 9.99999997475242708e-07f)
                {
                    stride_0 = stride_0 + u32(64);
                    continue;
                }
                var to_light_2 : vec3<f32> = to_light_1 / vec3<f32>(to_light_distance_0);
                var cone_0 : f32;
                if((_S26.w) == u32(2))
                {
                    cone_0 = spot_cone_0(to_light_2, _S25.axis_0.xyz, _S25.color_0.w, _S25.axis_0.w);
                }
                else
                {
                    cone_0 = 1.0f;
                }
                var u_0 : f32 = (2.0f * (f32(column_0) + 0.5f) / _S30 - 1.0f) * _S29;
                var v_0 : f32 = (2.0f * (f32(row_1) + 0.5f) / _S30 - 1.0f) * _S29;
                var axial_0 : f32 = u_0 * u_0 + v_0 * v_0 + 1.0f;
                gather_patch_0(&(bands_2), probe_1, _S22, _S33, sample_normal_1, (textureLoad((punctual_albedo_0), ((at_1)).xy, ((at_1)).z)).xyz * _S25.color_0.xyz * vec3<f32>(cone_0) * vec3<f32>(0.31830987334251404f), _S31 * _S31 / (axial_0 * sqrt(axial_0)) * to_light_distance_0 * to_light_distance_0 * punctual_falloff_0(to_light_distance_0, _S32.w));
                stride_0 = stride_0 + u32(64);
            }
            producer_0 = producer_0 + u32(1);
        }
    }
    tile_1[lane_0] = bands_2;
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
            tile_1[lane_0].r_0 = tile_1[lane_0].r_0 + tile_1[lane_0 + stride_0].r_0;
            tile_1[lane_0].g_0 = tile_1[lane_0].g_0 + tile_1[lane_0 + stride_0].g_0;
            tile_1[lane_0].b_0 = tile_1[lane_0].b_0 + tile_1[lane_0 + stride_0].b_0;
        }
        workgroupBarrier();
        stride_0 = (stride_0 >> (u32(1)));
    }
    var _S34 : bool;
    if(lane_0 == u32(0))
    {
        _S34 = probe_1 < (params_0.probes_0);
    }
    else
    {
        _S34 = false;
    }
    if(_S34)
    {
        var _S35 : u32 = probe_1 * u32(3);
        probes_1[_S35] = tile_1[i32(0)].r_0;
        probes_1[_S35 + u32(1)] = tile_1[i32(0)].g_0;
        probes_1[_S35 + u32(2)] = tile_1[i32(0)].b_0;
    }
    return;
}

