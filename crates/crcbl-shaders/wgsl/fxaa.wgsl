struct FxaaParams_std140_0
{
    @align(16) inv_source_0 : vec2<f32>,
    @align(8) edge_threshold_0 : f32,
    @align(4) edge_threshold_min_0 : f32,
    @align(16) subpixel_0 : f32,
};

@binding(2) @group(0) var<uniform> params_0 : FxaaParams_std140_0;
@binding(0) @group(0) var source_0 : texture_2d<f32>;

@binding(1) @group(0) var sourceSampler_0 : sampler;

var<private> SEARCH_STEP_0 : array<f32, i32(12)> = array<f32, i32(12)>( 1.0f, 1.0f, 1.0f, 1.0f, 1.0f, 1.5f, 2.0f, 2.0f, 2.0f, 2.0f, 4.0f, 8.0f );
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

fn tap_0( uv_1 : vec2<f32>,  offset_0 : vec2<f32>) -> vec3<f32>
{
    return (textureSampleLevel((source_0), (sourceSampler_0), (uv_1 + offset_0 * params_0.inv_source_0), (0.0f))).xyz;
}

fn luma_of_0( color_0 : vec3<f32>) -> f32
{
    return sqrt(dot(color_0, vec3<f32>(0.29899999499320984f, 0.58700001239776611f, 0.11400000005960464f)));
}

fn luma_at_0( uv_2 : vec2<f32>,  offset_1 : vec2<f32>) -> f32
{
    return luma_of_0(tap_0(uv_2, offset_1));
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_3 : vec2<f32>,
};

@fragment
fn fragmentMain( _S2 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var center_0 : vec3<f32> = tap_0(_S2.uv_3, vec2<f32>(0.0f, 0.0f));
    var luma_c_0 : f32 = luma_of_0(center_0);
    var luma_n_0 : f32 = luma_at_0(_S2.uv_3, vec2<f32>(0.0f, -1.0f));
    var luma_s_0 : f32 = luma_at_0(_S2.uv_3, vec2<f32>(0.0f, 1.0f));
    var luma_w_0 : f32 = luma_at_0(_S2.uv_3, vec2<f32>(-1.0f, 0.0f));
    var luma_e_0 : f32 = luma_at_0(_S2.uv_3, vec2<f32>(1.0f, 0.0f));
    var _S3 : f32 = max(luma_c_0, max(max(luma_n_0, luma_s_0), max(luma_w_0, luma_e_0)));
    var range_0 : f32 = _S3 - min(luma_c_0, min(min(luma_n_0, luma_s_0), min(luma_w_0, luma_e_0)));
    if(range_0 < (max(params_0.edge_threshold_min_0, _S3 * params_0.edge_threshold_0)))
    {
        var _S4 : pixelOutput_0 = pixelOutput_0( vec4<f32>(center_0, 1.0f) );
        return _S4;
    }
    var luma_nw_0 : f32 = luma_at_0(_S2.uv_3, vec2<f32>(-1.0f, -1.0f));
    var luma_ne_0 : f32 = luma_at_0(_S2.uv_3, vec2<f32>(1.0f, -1.0f));
    var luma_sw_0 : f32 = luma_at_0(_S2.uv_3, vec2<f32>(-1.0f, 1.0f));
    var luma_se_0 : f32 = luma_at_0(_S2.uv_3, vec2<f32>(1.0f, 1.0f));
    var luma_ns_0 : f32 = luma_n_0 + luma_s_0;
    var luma_we_0 : f32 = luma_w_0 + luma_e_0;
    var luma_wcorners_0 : f32 = luma_nw_0 + luma_sw_0;
    var luma_ecorners_0 : f32 = luma_ne_0 + luma_se_0;
    var _S5 : f32 = -2.0f * luma_c_0;
    var horizontal_0 : bool = (abs(-2.0f * luma_w_0 + luma_wcorners_0) + abs(_S5 + luma_ns_0) * 2.0f + abs(-2.0f * luma_e_0 + luma_ecorners_0)) >= (abs(-2.0f * luma_n_0 + (luma_nw_0 + luma_ne_0)) + abs(_S5 + luma_we_0) * 2.0f + abs(-2.0f * luma_s_0 + (luma_sw_0 + luma_se_0)));
    var luma_1_0 : f32;
    if(horizontal_0)
    {
        luma_1_0 = luma_n_0;
    }
    else
    {
        luma_1_0 = luma_w_0;
    }
    var luma_2_0 : f32;
    if(horizontal_0)
    {
        luma_2_0 = luma_s_0;
    }
    else
    {
        luma_2_0 = luma_e_0;
    }
    var _S6 : f32 = abs(luma_1_0 - luma_c_0);
    var _S7 : f32 = abs(luma_2_0 - luma_c_0);
    var steeper_1_0 : bool = _S6 >= _S7;
    var gradient_scaled_0 : f32 = 0.25f * max(_S6, _S7);
    var texel_0 : f32;
    if(horizontal_0)
    {
        texel_0 = params_0.inv_source_0.y;
    }
    else
    {
        texel_0 = params_0.inv_source_0.x;
    }
    var luma_local_0 : f32;
    if(steeper_1_0)
    {
        luma_local_0 = 0.5f * (luma_1_0 + luma_c_0);
    }
    else
    {
        luma_local_0 = 0.5f * (luma_2_0 + luma_c_0);
    }
    var step_length_0 : f32;
    if(steeper_1_0)
    {
        step_length_0 = - texel_0;
    }
    else
    {
        step_length_0 = texel_0;
    }
    var edge_uv_0 : vec2<f32> = _S2.uv_3;
    if(horizontal_0)
    {
        edge_uv_0[i32(1)] = edge_uv_0[i32(1)] + step_length_0 * 0.5f;
    }
    else
    {
        edge_uv_0[i32(0)] = edge_uv_0[i32(0)] + step_length_0 * 0.5f;
    }
    var along_0 : vec2<f32>;
    if(horizontal_0)
    {
        along_0 = vec2<f32>(params_0.inv_source_0.x, 0.0f);
    }
    else
    {
        along_0 = vec2<f32>(0.0f, params_0.inv_source_0.y);
    }
    var uv_neg_0 : vec2<f32> = edge_uv_0 - along_0;
    var uv_pos_0 : vec2<f32> = edge_uv_0 + along_0;
    var delta_neg_0 : f32 = luma_of_0((textureSampleLevel((source_0), (sourceSampler_0), (uv_neg_0), (0.0f))).xyz) - luma_local_0;
    var delta_pos_0 : f32 = luma_of_0((textureSampleLevel((source_0), (sourceSampler_0), (uv_pos_0), (0.0f))).xyz) - luma_local_0;
    var _S8 : bool = (abs(delta_neg_0)) >= gradient_scaled_0;
    var _S9 : bool = (abs(delta_pos_0)) >= gradient_scaled_0;
    if(horizontal_0)
    {
        luma_1_0 = _S2.uv_3.x - uv_neg_0.x;
    }
    else
    {
        luma_1_0 = _S2.uv_3.y - uv_neg_0.y;
    }
    if(horizontal_0)
    {
        luma_2_0 = uv_pos_0.x - _S2.uv_3.x;
    }
    else
    {
        luma_2_0 = uv_pos_0.y - _S2.uv_3.y;
    }
    var done_neg_0 : bool = _S8;
    var done_pos_0 : bool = _S9;
    var distance_neg_0 : f32 = luma_1_0;
    var distance_pos_0 : f32 = luma_2_0;
    var delta_neg_1 : f32 = delta_neg_0;
    var delta_pos_1 : f32 = delta_pos_0;
    var i_0 : i32 = i32(0);
    var uv_neg_1 : vec2<f32> = uv_neg_0;
    var uv_pos_1 : vec2<f32> = uv_pos_0;
    for(;;)
    {
        if(i_0 < i32(12))
        {
        }
        else
        {
            break;
        }
        var _S10 : i32 = i_0;
        if(!done_neg_0)
        {
            var uv_neg_2 : vec2<f32> = uv_neg_1 - along_0 * vec2<f32>(SEARCH_STEP_0[_S10]);
            var delta_neg_2 : f32 = luma_of_0((textureSampleLevel((source_0), (sourceSampler_0), (uv_neg_2), (0.0f))).xyz) - luma_local_0;
            var _S11 : bool = (abs(delta_neg_2)) >= gradient_scaled_0;
            if(horizontal_0)
            {
                luma_1_0 = _S2.uv_3.x - uv_neg_2.x;
            }
            else
            {
                luma_1_0 = _S2.uv_3.y - uv_neg_2.y;
            }
            done_neg_0 = _S11;
            distance_neg_0 = luma_1_0;
            delta_neg_1 = delta_neg_2;
            uv_neg_1 = uv_neg_2;
        }
        if(!done_pos_0)
        {
            var uv_pos_2 : vec2<f32> = uv_pos_1 + along_0 * vec2<f32>(SEARCH_STEP_0[_S10]);
            var delta_pos_2 : f32 = luma_of_0((textureSampleLevel((source_0), (sourceSampler_0), (uv_pos_2), (0.0f))).xyz) - luma_local_0;
            var _S12 : bool = (abs(delta_pos_2)) >= gradient_scaled_0;
            if(horizontal_0)
            {
                luma_1_0 = uv_pos_2.x - _S2.uv_3.x;
            }
            else
            {
                luma_1_0 = uv_pos_2.y - _S2.uv_3.y;
            }
            done_pos_0 = _S12;
            distance_pos_0 = luma_1_0;
            delta_pos_1 = delta_pos_2;
            uv_pos_1 = uv_pos_2;
        }
        i_0 = i_0 + i32(1);
    }
    var _S13 : f32 = max(0.0f, 0.5f - min(distance_neg_0, distance_pos_0) / max(distance_neg_0 + distance_pos_0, 9.99999997475242708e-07f)) * step_length_0;
    var delta_nearer_0 : f32;
    if(distance_neg_0 < distance_pos_0)
    {
        delta_nearer_0 = delta_neg_1;
    }
    else
    {
        delta_nearer_0 = delta_pos_1;
    }
    var offset_2 : f32;
    if(((luma_c_0 - luma_local_0) < 0.0f) == (delta_nearer_0 < 0.0f))
    {
        offset_2 = 0.0f;
    }
    else
    {
        offset_2 = _S13;
    }
    var subpixel_ratio_0 : f32 = saturate(abs((2.0f * (luma_ns_0 + luma_we_0) + luma_wcorners_0 + luma_ecorners_0) * 0.0833333358168602f - luma_c_0) / max(range_0, 9.99999997475242708e-07f));
    var subpixel_weight_0 : f32 = (-2.0f * subpixel_ratio_0 + 3.0f) * subpixel_ratio_0 * subpixel_ratio_0;
    var subpixel_offset_0 : f32 = subpixel_weight_0 * subpixel_weight_0 * params_0.subpixel_0 * step_length_0;
    var final_offset_0 : f32;
    if((abs(subpixel_offset_0)) > (abs(offset_2)))
    {
        final_offset_0 = subpixel_offset_0;
    }
    else
    {
        final_offset_0 = offset_2;
    }
    var result_uv_0 : vec2<f32> = _S2.uv_3;
    if(horizontal_0)
    {
        result_uv_0[i32(1)] = result_uv_0[i32(1)] + final_offset_0;
    }
    else
    {
        result_uv_0[i32(0)] = result_uv_0[i32(0)] + final_offset_0;
    }
    var _S14 : pixelOutput_0 = pixelOutput_0( vec4<f32>((textureSampleLevel((source_0), (sourceSampler_0), (result_uv_0), (0.0f))).xyz, 1.0f) );
    return _S14;
}

