struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct VolumetricParams_std140_0
{
    @align(16) inverse_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) eye_0 : vec4<f32>,
    @align(16) depth_row_0 : vec4<f32>,
    @align(16) fog_params_0 : vec4<f32>,
    @align(16) fog_color_0 : vec4<f32>,
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
@binding(1) @group(0) var<storage, read_write> volumetrics_0 : array<vec4<f32>>;

var<private> FOG_RATIO_KERNEL_0 : array<f32, i32(5)> = array<f32, i32(5)>( 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f );
var<private> FOG_KERNEL_0 : array<f32, i32(8)> = array<f32, i32(8)>( 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f );
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

fn fog_exp_neg_0( x_0 : f32) -> f32
{
    var clamped_0 : f32 = clamp(x_0, -87.0f, 87.0f);
    var n_0 : f32 = floor(clamped_0 * 1.4426950216293335f + 0.5f);
    var _S2 : f32 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);
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
        var _S3 : f32 = kernel_0 * _S2 + FOG_KERNEL_0[term_0];
        var term_1 : i32 = term_0 - i32(1);
        kernel_0 = _S3;
        term_0 = term_1;
    }
    return kernel_0 * (bitcast<f32>(((u32(i32(127) - i32(n_0)) << (u32(23))))));
}

fn fog_one_minus_exp_over_0( d_0 : f32) -> f32
{
    if((abs(d_0)) < 0.125f)
    {
        var _S4 : f32 = - d_0;
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
            var _S5 : f32 = series_0 * _S4 + FOG_RATIO_KERNEL_0[term_2];
            var term_3 : i32 = term_2 - i32(1);
            series_0 = _S5;
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

fn volumetric_slice_0( from_0 : vec3<f32>,  to_0 : vec3<f32>) -> vec4<f32>
{
    var reference_0 : f32 = params_0.fog_params_0.z;
    var survives_0 : f32 = fog_exp_neg_0(fog_optical_depth_0(params_0.fog_params_0.x, params_0.fog_params_0.y, from_0.y - reference_0, to_0.y - reference_0, length(to_0 - from_0)));
    return vec4<f32>(params_0.fog_color_0.xyz * vec3<f32>((1.0f - survives_0)), survives_0);
}

@compute
@workgroup_size(64, 1, 1)
fn scatterMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var froxel_0 : u32 = thread_0.x;
    var tiles_0 : u32 = max(params_0.grid_x_0, u32(1)) * max(params_0.grid_y_0, u32(1));
    var _S6 : u32 = max(params_0.slices_0, u32(1));
    var _S7 : bool;
    if(froxel_0 >= (tiles_0 * _S6))
    {
        _S7 = true;
    }
    else
    {
        _S7 = froxel_0 >= (params_0.froxel_count_0);
    }
    if(_S7)
    {
        return;
    }
    var tile_x_1 : u32 = froxel_0 % max(params_0.grid_x_0, u32(1));
    var _S8 : u32 = froxel_0 / max(params_0.grid_x_0, u32(1));
    var tile_y_1 : u32 = _S8 % max(params_0.grid_y_0, u32(1));
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
    var _S9 : u32 = slice_0 + u32(1);
    var to_depth_0 : f32;
    if(_S9 == _S6)
    {
        to_depth_0 = 1000.0f;
    }
    else
    {
        to_depth_0 = volumetric_slice_start_0(_S9);
    }
    volumetrics_0[froxel_0] = volumetric_slice_0(params_0.eye_0.xyz + along_0 * vec3<f32>(from_depth_0), params_0.eye_0.xyz + along_0 * vec3<f32>(to_depth_0));
    return;
}

@compute
@workgroup_size(64, 1, 1)
fn integrateMain(@builtin(global_invocation_id) thread_1 : vec3<u32>)
{
    var tile_0 : u32 = thread_1.x;
    var tiles_1 : u32 = max(params_0.grid_x_0, u32(1)) * max(params_0.grid_y_0, u32(1));
    if(tile_0 >= tiles_1)
    {
        return;
    }
    var _S10 : u32 = max(params_0.slices_0, u32(1));
    const _S11 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var slice_1 : u32 = u32(0);
    var accumulated_0 : vec3<f32> = _S11;
    var through_0 : f32 = 1.0f;
    for(;;)
    {
        if(slice_1 < _S10)
        {
        }
        else
        {
            break;
        }
        var froxel_1 : u32 = tile_0 + slice_1 * tiles_1;
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

