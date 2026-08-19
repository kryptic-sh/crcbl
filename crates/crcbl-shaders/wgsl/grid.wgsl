struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct GridParams_std140_0
{
    @align(16) inv_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) params_0 : vec4<f32>,
    @align(16) fine_color_0 : vec4<f32>,
    @align(16) coarse_color_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> grid_0 : GridParams_std140_0;
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

fn unproject_0( homogeneous_0 : vec4<f32>) -> vec3<f32>
{
    var _S2 : f32 = homogeneous_0.w;
    var w_0 : f32;
    if((abs(_S2)) < 9.99999997475242708e-07f)
    {
        w_0 = 9.99999997475242708e-07f;
    }
    else
    {
        w_0 = _S2;
    }
    return homogeneous_0.xyz / vec3<f32>(w_0);
}

fn line_coverage_0( cell_0 : f32,  derivative_0 : f32,  width_0 : f32) -> f32
{
    return saturate(width_0 * 0.5f + 0.5f - abs(fract(cell_0 - 0.5f) - 0.5f) / max(derivative_0, 9.99999993922529029e-09f));
}

fn resolvable_0( derivative_1 : f32,  width_1 : f32) -> f32
{
    return saturate(2.0f * (1.0f - width_1 * derivative_1));
}

fn scale_coverage_0( plane_0 : vec2<f32>,  spacing_0 : f32,  width_2 : f32) -> f32
{
    var cell_1 : vec2<f32> = plane_0 / vec2<f32>(spacing_0);
    var derivative_2 : vec2<f32> = (fwidth((cell_1)));
    var _S3 : f32 = derivative_2.x;
    var _S4 : f32 = derivative_2.y;
    return max(line_coverage_0(cell_1.x, _S3, width_2), line_coverage_0(cell_1.y, _S4, width_2)) * resolvable_0(max(_S3, _S4), width_2);
}

struct GridOutput_0
{
    @location(0) color_0 : vec4<f32>,
    @builtin(frag_depth) depth_0 : f32,
};

struct pixelInput_0
{
    @location(0) uv_1 : vec2<f32>,
};

@fragment
fn fragmentMain( _S5 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> GridOutput_0
{
    var ndc_0 : vec2<f32> = _S5.uv_1 * vec2<f32>(2.0f, -2.0f) + vec2<f32>(-1.0f, 1.0f);
    var origin_0 : vec3<f32> = unproject_0((((vec4<f32>(ndc_0, 1.0f, 1.0f)) * (mat4x4<f32>(grid_0.inv_view_proj_0.data_0[i32(0)][i32(0)], grid_0.inv_view_proj_0.data_0[i32(1)][i32(0)], grid_0.inv_view_proj_0.data_0[i32(2)][i32(0)], grid_0.inv_view_proj_0.data_0[i32(3)][i32(0)], grid_0.inv_view_proj_0.data_0[i32(0)][i32(1)], grid_0.inv_view_proj_0.data_0[i32(1)][i32(1)], grid_0.inv_view_proj_0.data_0[i32(2)][i32(1)], grid_0.inv_view_proj_0.data_0[i32(3)][i32(1)], grid_0.inv_view_proj_0.data_0[i32(0)][i32(2)], grid_0.inv_view_proj_0.data_0[i32(1)][i32(2)], grid_0.inv_view_proj_0.data_0[i32(2)][i32(2)], grid_0.inv_view_proj_0.data_0[i32(3)][i32(2)], grid_0.inv_view_proj_0.data_0[i32(0)][i32(3)], grid_0.inv_view_proj_0.data_0[i32(1)][i32(3)], grid_0.inv_view_proj_0.data_0[i32(2)][i32(3)], grid_0.inv_view_proj_0.data_0[i32(3)][i32(3)])))));
    var direction_0 : vec3<f32> = normalize(unproject_0((((vec4<f32>(ndc_0, 0.5f, 1.0f)) * (mat4x4<f32>(grid_0.inv_view_proj_0.data_0[i32(0)][i32(0)], grid_0.inv_view_proj_0.data_0[i32(1)][i32(0)], grid_0.inv_view_proj_0.data_0[i32(2)][i32(0)], grid_0.inv_view_proj_0.data_0[i32(3)][i32(0)], grid_0.inv_view_proj_0.data_0[i32(0)][i32(1)], grid_0.inv_view_proj_0.data_0[i32(1)][i32(1)], grid_0.inv_view_proj_0.data_0[i32(2)][i32(1)], grid_0.inv_view_proj_0.data_0[i32(3)][i32(1)], grid_0.inv_view_proj_0.data_0[i32(0)][i32(2)], grid_0.inv_view_proj_0.data_0[i32(1)][i32(2)], grid_0.inv_view_proj_0.data_0[i32(2)][i32(2)], grid_0.inv_view_proj_0.data_0[i32(3)][i32(2)], grid_0.inv_view_proj_0.data_0[i32(0)][i32(3)], grid_0.inv_view_proj_0.data_0[i32(1)][i32(3)], grid_0.inv_view_proj_0.data_0[i32(2)][i32(3)], grid_0.inv_view_proj_0.data_0[i32(3)][i32(3)]))))) - origin_0);
    var _S6 : f32 = direction_0.y;
    var crosses_0 : bool = (abs(_S6)) > 1.00000001168609742e-07f;
    var denominator_0 : f32;
    if(crosses_0)
    {
        denominator_0 = _S6;
    }
    else
    {
        denominator_0 = 1.00000001168609742e-07f;
    }
    var t_0 : f32 = - origin_0.y / denominator_0;
    var fade_distance_0 : f32 = grid_0.params_0.w;
    var visible_0 : bool;
    if(crosses_0)
    {
        visible_0 = t_0 > 0.0f;
    }
    else
    {
        visible_0 = false;
    }
    if(visible_0)
    {
        visible_0 = t_0 < fade_distance_0;
    }
    else
    {
        visible_0 = false;
    }
    var hit_0 : vec3<f32> = origin_0 + direction_0 * vec3<f32>(clamp(t_0, 0.0f, fade_distance_0));
    var spacing_1 : f32 = grid_0.params_0.x;
    var width_3 : f32 = grid_0.params_0.z;
    var _S7 : vec2<f32> = hit_0.xz;
    var fade_0 : f32 = saturate(1.0f - t_0 / fade_distance_0);
    var coarse_alpha_0 : f32 = grid_0.coarse_color_0.w * scale_coverage_0(_S7, spacing_1 * grid_0.params_0.y, width_3) * fade_0;
    var fine_alpha_0 : f32 = grid_0.fine_color_0.w * scale_coverage_0(_S7, spacing_1, width_3) * fade_0 * (1.0f - coarse_alpha_0);
    var alpha_0 : f32 = coarse_alpha_0 + fine_alpha_0;
    var color_1 : vec3<f32> = grid_0.coarse_color_0.xyz * vec3<f32>(coarse_alpha_0) + grid_0.fine_color_0.xyz * vec3<f32>(fine_alpha_0);
    var clip_0 : vec4<f32> = (((vec4<f32>(hit_0, 1.0f)) * (mat4x4<f32>(grid_0.view_proj_0.data_0[i32(0)][i32(0)], grid_0.view_proj_0.data_0[i32(1)][i32(0)], grid_0.view_proj_0.data_0[i32(2)][i32(0)], grid_0.view_proj_0.data_0[i32(3)][i32(0)], grid_0.view_proj_0.data_0[i32(0)][i32(1)], grid_0.view_proj_0.data_0[i32(1)][i32(1)], grid_0.view_proj_0.data_0[i32(2)][i32(1)], grid_0.view_proj_0.data_0[i32(3)][i32(1)], grid_0.view_proj_0.data_0[i32(0)][i32(2)], grid_0.view_proj_0.data_0[i32(1)][i32(2)], grid_0.view_proj_0.data_0[i32(2)][i32(2)], grid_0.view_proj_0.data_0[i32(3)][i32(2)], grid_0.view_proj_0.data_0[i32(0)][i32(3)], grid_0.view_proj_0.data_0[i32(1)][i32(3)], grid_0.view_proj_0.data_0[i32(2)][i32(3)], grid_0.view_proj_0.data_0[i32(3)][i32(3)]))));
    if(visible_0)
    {
        visible_0 = (clip_0.w) > 0.0f;
    }
    else
    {
        visible_0 = false;
    }
    if(visible_0)
    {
        visible_0 = alpha_0 > 0.0f;
    }
    else
    {
        visible_0 = false;
    }
    if(!visible_0)
    {
        discard;
    }
    var output_1 : GridOutput_0;
    output_1.color_0 = vec4<f32>(color_1, alpha_0);
    output_1.depth_0 = saturate(clip_0.z / clip_0.w);
    return output_1;
}

