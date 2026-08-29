@binding(2) @group(0) var scene_color_0 : texture_2d<f32>;

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
@binding(1) @group(0) var scene_depth_0 : texture_depth_2d;

@binding(3) @group(0) var<storage, read> volumetrics_0 : array<vec4<f32>>;

@binding(4) @group(0) var<storage, read> lighting_0 : array<vec4<f32>>;

var<private> FOG_RATIO_KERNEL_0 : array<f32, i32(5)> = array<f32, i32(5)>( 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f );
var<private> FOG_KERNEL_0 : array<f32, i32(8)> = array<f32, i32(8)>( 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f );
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

fn volumetric_unproject_0( ndc_0 : vec2<f32>,  depth_0 : f32) -> vec3<f32>
{
    var world_0 : vec4<f32> = (((vec4<f32>(ndc_0, depth_0, 1.0f)) * (mat4x4<f32>(params_0.inverse_view_proj_0.data_0[i32(0)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(0)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(1)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(2)], params_0.inverse_view_proj_0.data_0[i32(0)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(1)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(2)][i32(3)], params_0.inverse_view_proj_0.data_0[i32(3)][i32(3)]))));
    return world_0.xyz / vec3<f32>(world_0.w);
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

fn volumetric_phase_0( g_0 : f32,  cos_theta_0 : f32) -> f32
{
    var a_0 : f32 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    var _S6 : f32 = a_0 * a_0;
    var d_1 : f32 = 1.0f + _S6 - 2.0f * a_0 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S6) / (d_1 * sqrt(d_1));
}

fn volumetric_source_0( view_direction_0 : vec3<f32>,  lit_0 : vec4<f32>) -> vec3<f32>
{
    return params_0.fog_color_0.xyz + params_0.sun_radiance_0.xyz * vec3<f32>(volumetric_phase_0(params_0.sun_direction_0.w, dot(params_0.sun_direction_0.xyz, view_direction_0))) * vec3<f32>(lit_0.w) + lit_0.xyz;
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
fn fragmentMain( _S7 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S8 : vec2<i32> = vec2<i32>(position_1.xy);
    var _S9 : vec3<i32> = vec3<i32>(_S8, i32(0));
    var scene_0 : vec4<f32> = (textureLoad((scene_color_0), ((_S9)).xy, ((_S9)).z));
    var _S10 : u32 = max(params_0.grid_x_0, u32(1));
    var _S11 : u32 = max(params_0.grid_y_0, u32(1));
    var _S12 : u32 = max(params_0.slices_0, u32(1));
    var tiles_0 : u32 = _S10 * _S11;
    var _S13 : u32 = max(params_0.tile_pixels_0, u32(1));
    var _S14 : i32 = _S8.x;
    var _S15 : i32 = _S8.y;
    var ndc_1 : vec2<f32> = vec2<f32>((f32(_S14) + 0.5f) / f32(max(params_0.viewport_x_0, u32(1))) * 2.0f - 1.0f, 1.0f - (f32(_S15) + 0.5f) / f32(max(params_0.viewport_y_0, u32(1))) * 2.0f);
    var _S16 : f32 = (textureLoad((scene_depth_0), ((_S9)).xy, ((_S9)).z));
    var view_depth_0 : f32;
    if(_S16 > 0.0f)
    {
        view_depth_0 = dot(params_0.depth_row_0, vec4<f32>(volumetric_unproject_0(ndc_1, _S16), 1.0f));
    }
    else
    {
        view_depth_0 = 1000.0f;
    }
    var view_depth_1 : f32 = clamp(view_depth_0, 0.0f, 1000.0f);
    var slice_start_0 : f32 = 0.0f;
    var slice_0 : u32 = u32(0);
    var next_start_0 : f32 = 0.14677993953227997f;
    for(;;)
    {
        var _S17 : u32 = slice_0 + u32(1);
        var _S18 : bool;
        if(_S17 < _S12)
        {
            _S18 = next_start_0 <= view_depth_1;
        }
        else
        {
            _S18 = false;
        }
        if(_S18)
        {
        }
        else
        {
            break;
        }
        var next_start_1 : f32 = next_start_0 * 1.46779930591583252f;
        slice_start_0 = next_start_0;
        next_start_0 = next_start_1;
        slice_0 = _S17;
    }
    var _S19 : u32 = u32(max(_S14, i32(0))) / _S13;
    var _S20 : u32 = min(_S19, _S10 - u32(1));
    var _S21 : u32 = u32(max(_S15, i32(0))) / _S13;
    var froxel_0 : u32 = _S20 + min(_S21, _S11 - u32(1)) * _S10 + slice_0 * tiles_0;
    if(froxel_0 >= (params_0.froxel_count_0))
    {
        var _S22 : pixelOutput_0 = pixelOutput_0( scene_0 );
        return _S22;
    }
    var prefix_0 : vec4<f32> = volumetrics_0[froxel_0];
    var near_point_0 : vec3<f32> = volumetric_unproject_0(ndc_1, 1.0f);
    var along_0 : vec3<f32> = (near_point_0 - params_0.eye_0.xyz) / vec3<f32>(max(dot(params_0.depth_row_0, vec4<f32>(near_point_0, 1.0f)), 9.99999997475242708e-07f));
    var from_0 : vec3<f32> = params_0.eye_0.xyz + along_0 * vec3<f32>(slice_start_0);
    var to_0 : vec3<f32> = params_0.eye_0.xyz + along_0 * vec3<f32>(max(view_depth_1, slice_start_0));
    var reference_0 : f32 = params_0.fog_params_0.z;
    var segment_0 : vec3<f32> = to_0 - from_0;
    var length_of_0 : f32 = length(segment_0);
    var partial_survives_0 : f32 = fog_exp_neg_0(fog_optical_depth_0(params_0.fog_params_0.x, params_0.fog_params_0.y, from_0.y - reference_0, to_0.y - reference_0, length_of_0));
    var view_direction_1 : vec3<f32>;
    if(length_of_0 > 9.99999997475242708e-07f)
    {
        view_direction_1 = segment_0 / vec3<f32>(length_of_0);
    }
    else
    {
        view_direction_1 = vec3<f32>(0.0f, 0.0f, 1.0f);
    }
    var _S23 : f32 = prefix_0.w;
    var _S24 : pixelOutput_0 = pixelOutput_0( vec4<f32>(scene_0.xyz * vec3<f32>((_S23 * partial_survives_0)) + prefix_0.xyz + vec3<f32>(_S23) * (volumetric_source_0(view_direction_1, lighting_0[froxel_0]) * vec3<f32>((1.0f - partial_survives_0))), scene_0.w) );
    return _S24;
}

