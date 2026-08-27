struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0
{
    @align(16) data_1 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(2)>,
};

struct VolumetricParams_std140_0
{
    @align(16) inverse_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) eye_0 : vec4<f32>,
    @align(16) depth_row_0 : vec4<f32>,
    @align(16) fog_params_0 : vec4<f32>,
    @align(16) fog_color_0 : vec4<f32>,
    @align(16) sun_direction_0 : vec4<f32>,
    @align(16) sun_radiance_0 : vec4<f32>,
    @align(16) shadow_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0,
    @align(16) cascade_far_0 : vec4<f32>,
    @align(16) shadow_params_0 : vec4<f32>,
    @align(16) grid_x_0 : u32,
    @align(4) grid_y_0 : u32,
    @align(8) slices_0 : u32,
    @align(4) tile_pixels_0 : u32,
    @align(16) viewport_x_0 : u32,
    @align(4) viewport_y_0 : u32,
    @align(8) froxel_count_0 : u32,
    @align(4) pad0_0 : u32,
};

@binding(0) @group(0) var<uniform> params_0 : VolumetricParams_std140_0;
@binding(2) @group(0) var shadow_atlas_0 : texture_depth_2d;

@binding(3) @group(0) var shadow_sampler_0 : sampler_comparison;

@binding(4) @group(0) var<storage, read_write> visibilities_0 : array<f32>;

@binding(1) @group(0) var<storage, read_write> volumetrics_0 : array<vec4<f32>>;

var<private> FOG_RATIO_KERNEL_0 : array<f32, i32(5)> = array<f32, i32(5)>( 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f );
var<private> FOG_KERNEL_0 : array<f32, i32(8)> = array<f32, i32(8)>( 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f );
var<private> SHADOW_DISC_0 : array<vec2<f32>, i32(32)> = array<vec2<f32>, i32(32)>( vec2<f32>(0.125f, 0.0f), vec2<f32>(-0.15964500606060028f, 0.14624799787998199f), vec2<f32>(0.02443600073456764f, -0.27843800187110901f), vec2<f32>(0.2012220025062561f, 0.26245900988578796f), vec2<f32>(-0.36926800012588501f, -0.06531800329685211f), vec2<f32>(0.34980198740959167f, -0.22251600027084351f), vec2<f32>(-0.11700200289487839f, 0.43524199724197388f), vec2<f32>(-0.22313599288463593f, -0.42963400483131409f), vec2<f32>(0.48411500453948975f, 0.17679800093173981f), vec2<f32>(-0.50364100933074951f, 0.20789599418640137f), vec2<f32>(0.24278800189495087f, -0.51882398128509521f), vec2<f32>(0.17941400408744812f, 0.57200098037719727f), vec2<f32>(-0.54075700044631958f, -0.31338000297546387f), vec2<f32>(0.63437002897262573f, -0.13946400582790375f), vec2<f32>(-0.38714599609375f, 0.55067497491836548f), vec2<f32>(-0.0894400030374527f, -0.69019997119903564f), vec2<f32>(0.5490720272064209f, 0.46275800466537476f), vec2<f32>(-0.73887801170349121f, 0.0305550005286932f), vec2<f32>(0.5389549732208252f, -0.53633201122283936f), vec2<f32>(-0.03605800122022629f, 0.77979201078414917f), vec2<f32>(-0.51281797885894775f, -0.61452698707580566f), vec2<f32>(0.81235998868942261f, 0.10930199921131134f), vec2<f32>(-0.68831098079681396f, 0.47890898585319519f), vec2<f32>(0.18808600306510925f, -0.83606100082397461f), vec2<f32>(0.43503299355506897f, 0.75919097661972046f), vec2<f32>(-0.85044801235198975f, -0.27131599187850952f), vec2<f32>(0.82610201835632324f, -0.38168001174926758f), vec2<f32>(-0.35788801312446594f, 0.85515600442886353f), vec2<f32>(-0.31940698623657227f, -0.88803398609161377f), vec2<f32>(0.84990900754928589f, 0.44668799638748169f), vec2<f32>(-0.94403499364852905f, 0.24884499609470367f), vec2<f32>(0.53659600019454956f, -0.83452999591827393f) );
var<private> SHADOW_ROTATIONS_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(1.0f, 0.0f), vec2<f32>(0.92387998104095459f, 0.38268300890922546f), vec2<f32>(0.70710700750350952f, 0.70710700750350952f), vec2<f32>(0.38268300890922546f, 0.92387998104095459f), vec2<f32>(0.0f, 1.0f), vec2<f32>(-0.38268300890922546f, 0.92387998104095459f), vec2<f32>(-0.70710700750350952f, 0.70710700750350952f), vec2<f32>(-0.92387998104095459f, 0.38268300890922546f), vec2<f32>(-1.0f, 0.0f), vec2<f32>(-0.92387998104095459f, -0.38268300890922546f), vec2<f32>(-0.70710700750350952f, -0.70710700750350952f), vec2<f32>(-0.38268300890922546f, -0.92387998104095459f), vec2<f32>(-0.0f, -1.0f), vec2<f32>(0.38268300890922546f, -0.92387998104095459f), vec2<f32>(0.70710700750350952f, -0.70710700750350952f), vec2<f32>(0.92387998104095459f, -0.38268300890922546f) );
var<private> SHADOW_DITHER_0 : array<u32, i32(16)> = array<u32, i32(16)>( u32(0), u32(8), u32(2), u32(10), u32(12), u32(4), u32(14), u32(6), u32(3), u32(11), u32(1), u32(9), u32(15), u32(7), u32(13), u32(5) );
fn volumetric_unproject_0( ndc_0 : vec2<f32>,  depth_0 : f32) -> vec3<f32>
{
    var world_0 : vec4<f32> = (((vec4<f32>(ndc_0, depth_0, 1.0f)) * (mat4x4<f32>(params_0.inverse_view_proj_0.data_0[i32(0)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(3)]))));
    return world_0.xyz / vec3<f32>(world_0.w);
}

fn volumetric_tile_ray_0( tile_x_0 : u32,  tile_y_0 : u32,  near_point_0 : ptr<function, vec3<f32>>,  near_depth_0 : ptr<function, f32>)
{
    var pixel_0 : vec2<f32> = (vec2<f32>(f32(tile_x_0), f32(tile_y_0)) + vec2<f32>(0.5f)) * vec2<f32>(f32(params_0.tile_pixels_0));
    var _S1 : vec3<f32> = volumetric_unproject_0(vec2<f32>(pixel_0.x / f32(max(params_0.viewport_x_0, u32(1))) * 2.0f - 1.0f, 1.0f - pixel_0.y / f32(max(params_0.viewport_y_0, u32(1))) * 2.0f), 1.0f);
    (*near_point_0) = _S1;
    (*near_depth_0) = max(dot(params_0.depth_row_0, vec4<f32>(_S1, 1.0f)), 9.99999997475242708e-07f);
    return;
}

fn volumetric_slice_start_0( index_0 : u32) -> f32
{
    var step_0 : u32 = u32(0);
    var start_0 : f32 = 0.10000000149011612f;
    for(;;)
    {
        if(step_0 < index_0)
        {
        }
        else
        {
            break;
        }
        var start_1 : f32 = start_0 * 1.46779930591583252f;
        step_0 = step_0 + u32(1);
        start_0 = start_1;
    }
    return start_0;
}

fn atlas_uv_0( tile_0 : u32,  tile_uv_0 : vec2<f32>) -> vec2<f32>
{
    return (vec2<f32>(f32(tile_0 % u32(4)), f32(tile_0 / u32(4))) + tile_uv_0) / vec2<f32>(4.0f, 4.0f);
}

fn tile_pcf_0( tile_1 : u32,  tile_uv_1 : vec2<f32>,  reference_0 : f32,  pixel_1 : vec2<f32>) -> f32
{
    var texel_0 : vec2<f32> = params_0.shadow_params_0.xy;
    const grid_0 : vec2<f32> = vec2<f32>(4.0f, 4.0f);
    var _S2 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_0 * grid_0;
    var cell_0 : vec2<u32> = (vec2<u32>(pixel_1) & (vec2<u32>(u32(3))));
    var _S3 : u32 = cell_0.y * u32(4) + cell_0.x;
    var index_1 : u32 = u32(0);
    var visibility_0 : f32 = 0.0f;
    for(;;)
    {
        if(index_1 < u32(32))
        {
        }
        else
        {
            break;
        }
        var spoke_0 : vec2<f32> = SHADOW_DISC_0[index_1] * vec2<f32>(2.0f);
        var _S4 : f32 = spoke_0.x;
        var _S5 : f32 = SHADOW_ROTATIONS_0[SHADOW_DITHER_0[_S3]].x;
        var _S6 : f32 = spoke_0.y;
        var _S7 : f32 = SHADOW_ROTATIONS_0[SHADOW_DITHER_0[_S3]].y;
        var visibility_1 : f32 = visibility_0 + (textureSampleCompareLevel((shadow_atlas_0), (shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + vec2<f32>(_S4 * _S5 - _S6 * _S7, _S4 * _S7 + _S6 * _S5) * texel_0 * grid_0, _S2, vec2<f32>(1.0f) - _S2))), (reference_0)));
        index_1 = index_1 + u32(1);
        visibility_0 = visibility_1;
    }
    return visibility_0 / 32.0f;
}

fn volumetric_sun_visibility_0( world_position_0 : vec3<f32>,  pixel_2 : vec2<f32>) -> f32
{
    var cascade_0 : u32;
    var _S8 : f32 = length(world_position_0 - params_0.eye_0.xyz);
    var index_2 : u32 = u32(0);
    for(;;)
    {
        if(index_2 < u32(2))
        {
        }
        else
        {
            cascade_0 = u32(1);
            break;
        }
        if(_S8 < (params_0.cascade_far_0[index_2]))
        {
            cascade_0 = index_2;
            break;
        }
        index_2 = index_2 + u32(1);
    }
    var clip_0 : vec4<f32> = (((vec4<f32>(world_position_0, 1.0f)) * (mat4x4<f32>(params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(0)][i32(0)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(1)][i32(0)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(2)][i32(0)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(3)][i32(0)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(0)][i32(1)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(1)][i32(1)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(2)][i32(1)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(3)][i32(1)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(0)][i32(2)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(1)][i32(2)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(2)][i32(2)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(3)][i32(2)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(0)][i32(3)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(1)][i32(3)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(2)][i32(3)], params_0.shadow_view_proj_0.data_1[cascade_0].data_0[i32(3)][i32(3)]))));
    var ndc_1 : vec3<f32> = clip_0.xyz / vec3<f32>(clip_0.w);
    var _S9 : bool;
    if((any(((abs(ndc_1.xy)) > vec2<f32>(1.0f)))))
    {
        _S9 = true;
    }
    else
    {
        _S9 = (ndc_1.z) <= 0.0f;
    }
    if(_S9)
    {
        return 1.0f;
    }
    return tile_pcf_0(cascade_0, vec2<f32>(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_2);
}

fn fog_exp_neg_0( x_0 : f32) -> f32
{
    var clamped_0 : f32 = clamp(x_0, -87.0f, 87.0f);
    var n_0 : f32 = floor(clamped_0 * 1.4426950216293335f + 0.5f);
    var _S10 : f32 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);
    var kernel_0 : f32 = 0.0001984127011383f;
    var term_0 : i32 = i32(6);
    for(;;)
    {
        if(term_0 >= i32(0))
        {
        }
        else
        {
            break;
        }
        var _S11 : f32 = kernel_0 * _S10 + FOG_KERNEL_0[term_0];
        var term_1 : i32 = term_0 - i32(1);
        kernel_0 = _S11;
        term_0 = term_1;
    }
    return kernel_0 * (bitcast<f32>(((u32(i32(127) - i32(n_0)) << (u32(23))))));
}

fn fog_one_minus_exp_over_0( d_0 : f32) -> f32
{
    if((abs(d_0)) < 0.125f)
    {
        var _S12 : f32 = - d_0;
        var series_0 : f32 = 0.00833333376795053f;
        var term_2 : i32 = i32(3);
        for(;;)
        {
            if(term_2 >= i32(0))
            {
            }
            else
            {
                break;
            }
            var _S13 : f32 = series_0 * _S12 + FOG_RATIO_KERNEL_0[term_2];
            var term_3 : i32 = term_2 - i32(1);
            series_0 = _S13;
            term_2 = term_3;
        }
        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}

fn fog_optical_depth_0( density_0 : f32,  falloff_0 : f32,  height_a_0 : f32,  height_b_0 : f32,  distance_0 : f32) -> f32
{
    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_0, 0.0f, 32.0f);
    }
    return clamp(density_0 * distance_0 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}

fn volumetric_phase_0( g_0 : f32,  cos_theta_0 : f32) -> f32
{
    var a_0 : f32 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    var _S14 : f32 = a_0 * a_0;
    var d_1 : f32 = 1.0f + _S14 - 2.0f * a_0 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S14) / (d_1 * sqrt(d_1));
}

fn volumetric_source_0( view_direction_0 : vec3<f32>,  visibility_2 : f32) -> vec3<f32>
{
    return params_0.fog_color_0.xyz + params_0.sun_radiance_0.xyz * vec3<f32>(volumetric_phase_0(params_0.sun_direction_0.w, dot(params_0.sun_direction_0.xyz, view_direction_0))) * vec3<f32>(visibility_2);
}

fn volumetric_slice_0( from_0 : vec3<f32>,  to_0 : vec3<f32>,  visibility_3 : f32) -> vec4<f32>
{
    var reference_1 : f32 = params_0.fog_params_0.z;
    var segment_0 : vec3<f32> = to_0 - from_0;
    var length_of_0 : f32 = length(segment_0);
    var survives_0 : f32 = fog_exp_neg_0(fog_optical_depth_0(params_0.fog_params_0.x, params_0.fog_params_0.y, from_0.y - reference_1, to_0.y - reference_1, length_of_0));
    var view_direction_1 : vec3<f32>;
    if(length_of_0 > 9.99999997475242708e-07f)
    {
        view_direction_1 = segment_0 / vec3<f32>(length_of_0);
    }
    else
    {
        view_direction_1 = vec3<f32>(0.0f, 0.0f, 1.0f);
    }
    return vec4<f32>(volumetric_source_0(view_direction_1, visibility_3) * vec3<f32>((1.0f - survives_0)), survives_0);
}

@compute
@workgroup_size(64, 1, 1)
fn scatterMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var froxel_0 : u32 = thread_0.x;
    var tiles_0 : u32 = max(params_0.grid_x_0, u32(1)) * max(params_0.grid_y_0, u32(1));
    var _S15 : u32 = max(params_0.slices_0, u32(1));
    var _S16 : bool;
    if(froxel_0 >= (tiles_0 * _S15))
    {
        _S16 = true;
    }
    else
    {
        _S16 = froxel_0 >= (params_0.froxel_count_0);
    }
    if(_S16)
    {
        return;
    }
    var tile_x_1 : u32 = froxel_0 % max(params_0.grid_x_0, u32(1));
    var _S17 : u32 = froxel_0 / max(params_0.grid_x_0, u32(1));
    var tile_y_1 : u32 = _S17 % max(params_0.grid_y_0, u32(1));
    var slice_0 : u32 = froxel_0 / tiles_0;
    var near_point_1 : vec3<f32>;
    var near_depth_1 : f32;
    volumetric_tile_ray_0(tile_x_1, tile_y_1, &(near_point_1), &(near_depth_1));
    var along_0 : vec3<f32> = (near_point_1 - params_0.eye_0.xyz) / vec3<f32>(near_depth_1);
    var from_depth_0 : f32;
    if(slice_0 == u32(0))
    {
        from_depth_0 = 0.0f;
    }
    else
    {
        from_depth_0 = volumetric_slice_start_0(slice_0);
    }
    var _S18 : u32 = slice_0 + u32(1);
    var to_depth_0 : f32;
    if(_S18 == _S15)
    {
        to_depth_0 = 1000.0f;
    }
    else
    {
        to_depth_0 = volumetric_slice_start_0(_S18);
    }
    var from_1 : vec3<f32> = params_0.eye_0.xyz + along_0 * vec3<f32>(from_depth_0);
    var to_1 : vec3<f32> = params_0.eye_0.xyz + along_0 * vec3<f32>(to_depth_0);
    var visibility_4 : f32 = volumetric_sun_visibility_0((from_1 + to_1) * vec3<f32>(0.5f), vec2<f32>(f32(tile_x_1), f32(tile_y_1)));
    visibilities_0[froxel_0] = visibility_4;
    volumetrics_0[froxel_0] = volumetric_slice_0(from_1, to_1, visibility_4);
    return;
}

@compute
@workgroup_size(64, 1, 1)
fn integrateMain(@builtin(global_invocation_id) thread_1 : vec3<u32>)
{
    var tile_2 : u32 = thread_1.x;
    var tiles_1 : u32 = max(params_0.grid_x_0, u32(1)) * max(params_0.grid_y_0, u32(1));
    if(tile_2 >= tiles_1)
    {
        return;
    }
    var _S19 : u32 = max(params_0.slices_0, u32(1));
    const _S20 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var slice_1 : u32 = u32(0);
    var accumulated_0 : vec3<f32> = _S20;
    var through_0 : f32 = 1.0f;
    for(;;)
    {
        if(slice_1 < _S19)
        {
        }
        else
        {
            break;
        }
        var froxel_1 : u32 = tile_2 + slice_1 * tiles_1;
        if(froxel_1 >= (params_0.froxel_count_0))
        {
            break;
        }
        var own_0 : vec4<f32> = volumetrics_0[froxel_1];
        volumetrics_0[froxel_1] = vec4<f32>(accumulated_0, through_0);
        var accumulated_1 : vec3<f32> = accumulated_0 + vec3<f32>(through_0) * own_0.xyz;
        var through_1 : f32 = through_0 * own_0.w;
        slice_1 = slice_1 + u32(1);
        accumulated_0 = accumulated_1;
        through_0 = through_1;
    }
    return;
}

