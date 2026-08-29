struct SmaaParams_std140_0
{
    @align(16) inv_source_0 : vec2<f32>,
    @align(8) source_size_0 : vec2<f32>,
};

@binding(4) @group(0) var<uniform> params_0 : SmaaParams_std140_0;
@binding(0) @group(0) var edges_0 : texture_2d<f32>;

@binding(3) @group(0) var tableSampler_0 : sampler;

@binding(1) @group(0) var area_0 : texture_2d<f32>;

@binding(2) @group(0) var search_0 : texture_2d<f32>;

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

fn sample_edges_0( uv_1 : vec2<f32>) -> vec4<f32>
{
    return (textureSampleLevel((edges_0), (tableSampler_0), (uv_1), (0.0f)));
}

fn search_diag1_0( uv_2 : vec2<f32>,  dir_0 : vec2<f32>,  e_0 : ptr<function, vec2<f32>>) -> vec2<f32>
{
    var _S2 : vec3<f32> = vec3<f32>(uv_2, -1.0f);
    (*e_0) = vec2<f32>(0.0f, 0.0f);
    var weight_0 : f32 = 1.0f;
    var i_0 : i32 = i32(0);
    var coord_0 : vec3<f32> = _S2;
    for(;;)
    {
        if(i_0 < i32(8))
        {
        }
        else
        {
            break;
        }
        var _S3 : bool;
        if((coord_0.z) < 7.0f)
        {
            _S3 = weight_0 > 0.89999997615814209f;
        }
        else
        {
            _S3 = false;
        }
        if(_S3)
        {
            var coord_1 : vec3<f32> = coord_0 + vec3<f32>(dir_0 * params_0.inv_source_0, 1.0f);
            var _S4 : vec2<f32> = sample_edges_0(coord_1.xy).xy;
            (*e_0) = _S4;
            weight_0 = dot(_S4, vec2<f32>(0.5f, 0.5f));
            coord_0 = coord_1;
        }
        i_0 = i_0 + i32(1);
    }
    return vec2<f32>(coord_0.z, weight_0);
}

fn sample_edges_at_0( uv_3 : vec2<f32>,  offset_0 : vec2<f32>) -> vec4<f32>
{
    return (textureSampleLevel((edges_0), (tableSampler_0), (uv_3 + offset_0 * params_0.inv_source_0), (0.0f)));
}

fn decode_diag_bilinear4_0( e_1 : vec4<f32>) -> vec4<f32>
{
    var _S5 : vec4<f32> = e_1;
    var _S6 : f32 = e_1.x;
    _S5[i32(0)] = _S6 * abs(5.0f * _S6 - 3.75f);
    _S5[i32(2)] = _S5.z * abs(5.0f * _S5.z - 3.75f);
    return round(_S5);
}

fn area_diag_0( dist_0 : vec2<f32>,  e_2 : vec2<f32>) -> vec2<f32>
{
    var texcoord_0 : vec2<f32> = vec2<f32>(0.00625000009313226f, 0.01250000018626451f) * (vec2<f32>(20.0f, 20.0f) * e_2 + dist_0) + vec2<f32>(0.5f) * vec2<f32>(0.00625000009313226f, 0.01250000018626451f);
    texcoord_0[i32(0)] = texcoord_0[i32(0)] + 0.5f;
    return (textureSampleLevel((area_0), (tableSampler_0), (texcoord_0), (0.0f))).xy;
}

fn decode_diag_bilinear_0( e_3 : vec2<f32>) -> vec2<f32>
{
    var _S7 : vec2<f32> = e_3;
    var _S8 : f32 = e_3.x;
    _S7[i32(0)] = _S8 * abs(5.0f * _S8 - 3.75f);
    return round(_S7);
}

fn search_diag2_0( uv_4 : vec2<f32>,  dir_1 : vec2<f32>,  e_4 : ptr<function, vec2<f32>>) -> vec2<f32>
{
    var coord_2 : vec3<f32> = vec3<f32>(uv_4, -1.0f);
    coord_2[i32(0)] = coord_2[i32(0)] + 0.25f * params_0.inv_source_0.x;
    (*e_4) = vec2<f32>(0.0f, 0.0f);
    var weight_1 : f32 = 1.0f;
    var i_1 : i32 = i32(0);
    for(;;)
    {
        if(i_1 < i32(8))
        {
        }
        else
        {
            break;
        }
        var _S9 : bool;
        if((coord_2.z) < 7.0f)
        {
            _S9 = weight_1 > 0.89999997615814209f;
        }
        else
        {
            _S9 = false;
        }
        if(_S9)
        {
            var _S10 : vec3<f32> = coord_2 + vec3<f32>(dir_1 * params_0.inv_source_0, 1.0f);
            coord_2 = _S10;
            var _S11 : vec2<f32> = decode_diag_bilinear_0(sample_edges_0(_S10.xy).xy);
            (*e_4) = _S11;
            weight_1 = dot(_S11, vec2<f32>(0.5f, 0.5f));
        }
        i_1 = i_1 + i32(1);
    }
    return vec2<f32>(coord_2.z, weight_1);
}

fn calculate_diag_weights_0( uv_5 : vec2<f32>,  e_5 : vec2<f32>) -> vec2<f32>
{
    const weights_0 : vec2<f32> = vec2<f32>(0.0f, 0.0f);
    var d_0 : vec4<f32>;
    var end_0 : vec2<f32>;
    if((e_5.x) > 0.0f)
    {
        var found_0 : vec2<f32> = search_diag1_0(uv_5, vec2<f32>(-1.0f, 1.0f), &(end_0));
        var _S12 : f32 = found_0.x;
        d_0[i32(2)] = found_0.y;
        d_0[i32(0)] = _S12 + f32((end_0.y) > 0.89999997615814209f);
    }
    else
    {
        d_0[i32(0)] = 0.0f;
        d_0[i32(2)] = 0.0f;
    }
    var found_positive_0 : vec2<f32> = search_diag1_0(uv_5, vec2<f32>(1.0f, -1.0f), &(end_0));
    d_0[i32(1)] = found_positive_0.x;
    d_0[i32(3)] = found_positive_0.y;
    var weights_1 : vec2<f32>;
    if((d_0.x + d_0.y) > 2.0f)
    {
        var coords_0 : vec4<f32> = vec4<f32>(- d_0.x + 0.25f, d_0.x, d_0.y, - d_0.y - 0.25f) * vec4<f32>(params_0.inv_source_0, params_0.inv_source_0) + vec4<f32>(uv_5, uv_5);
        var _S13 : vec2<f32> = sample_edges_at_0(coords_0.xy, vec2<f32>(-1.0f, 0.0f)).xy;
        var fetched_0 : vec4<f32>;
        fetched_0.x = _S13.x;
        fetched_0.y = _S13.y;
        var _S14 : vec2<f32> = sample_edges_at_0(coords_0.zw, vec2<f32>(1.0f, 0.0f)).xy;
        fetched_0.z = _S14.x;
        fetched_0.w = _S14.y;
        var decoded_0 : vec4<f32> = decode_diag_bilinear4_0(fetched_0);
        var crossing_0 : vec2<f32> = vec2<f32>(2.0f, 2.0f) * vec2<f32>(decoded_0.y, decoded_0.w) + vec2<f32>(decoded_0.x, decoded_0.z);
        if((d_0.z) >= 0.89999997615814209f)
        {
            crossing_0[i32(0)] = 0.0f;
        }
        if((d_0.w) >= 0.89999997615814209f)
        {
            crossing_0[i32(1)] = 0.0f;
        }
        weights_1 = area_diag_0(d_0.xy, crossing_0);
    }
    else
    {
        weights_1 = weights_0;
    }
    var found_negative_0 : vec2<f32> = search_diag2_0(uv_5, vec2<f32>(-1.0f, -1.0f), &(end_0));
    d_0[i32(0)] = found_negative_0.x;
    d_0[i32(2)] = found_negative_0.y;
    const _S15 : vec2<f32> = vec2<f32>(1.0f, 0.0f);
    if((sample_edges_at_0(uv_5, _S15).x) > 0.0f)
    {
        var found_1 : vec2<f32> = search_diag2_0(uv_5, vec2<f32>(1.0f, 1.0f), &(end_0));
        var _S16 : f32 = found_1.x;
        d_0[i32(3)] = found_1.y;
        d_0[i32(1)] = _S16 + f32((end_0.y) > 0.89999997615814209f);
    }
    else
    {
        d_0[i32(1)] = 0.0f;
        d_0[i32(3)] = 0.0f;
    }
    if((d_0.x + d_0.y) > 2.0f)
    {
        var coords_1 : vec4<f32> = vec4<f32>(- d_0.x, - d_0.x, d_0.y, d_0.y) * vec4<f32>(params_0.inv_source_0, params_0.inv_source_0) + vec4<f32>(uv_5, uv_5);
        var c_0 : vec4<f32>;
        var _S17 : vec2<f32> = coords_1.xy;
        c_0[i32(0)] = sample_edges_at_0(_S17, vec2<f32>(-1.0f, 0.0f)).y;
        c_0[i32(1)] = sample_edges_at_0(_S17, vec2<f32>(0.0f, -1.0f)).x;
        var far_0 : vec2<f32> = sample_edges_at_0(coords_1.zw, _S15).xy;
        c_0[i32(2)] = far_0.y;
        c_0[i32(3)] = far_0.x;
        var crossing_1 : vec2<f32> = vec2<f32>(2.0f, 2.0f) * c_0.xz + c_0.yw;
        if((d_0.z) >= 0.89999997615814209f)
        {
            crossing_1[i32(0)] = 0.0f;
        }
        if((d_0.w) >= 0.89999997615814209f)
        {
            crossing_1[i32(1)] = 0.0f;
        }
        var found_area_0 : vec2<f32> = area_diag_0(d_0.xy, crossing_1);
        weights_1 = weights_1 + vec2<f32>(found_area_0.y, found_area_0.x);
    }
    return weights_1;
}

fn search_length_0( e_6 : vec2<f32>,  offset_1 : f32) -> f32
{
    var _S18 : vec2<f32> = vec2<f32>(1.0f);
    return (textureSampleLevel((search_0), (tableSampler_0), ((vec2<f32>(66.0f, 33.0f) * vec2<f32>(0.5f, -1.0f) + vec2<f32>(-1.0f, 1.0f)) * (_S18 / vec2<f32>(64.0f, 16.0f)) * e_6 + (vec2<f32>(66.0f, 33.0f) * vec2<f32>(offset_1, 1.0f) + vec2<f32>(0.5f, -0.5f)) * (_S18 / vec2<f32>(64.0f, 16.0f))), (0.0f))).x;
}

fn search_x_left_0( uv_6 : vec2<f32>) -> f32
{
    var e_7 : vec2<f32> = vec2<f32>(0.0f, 1.0f);
    var i_2 : i32 = i32(0);
    var coord_3 : vec2<f32> = uv_6;
    for(;;)
    {
        if(i_2 < i32(16))
        {
        }
        else
        {
            break;
        }
        var _S19 : bool;
        if((e_7.y) > 0.82810002565383911f)
        {
            _S19 = (e_7.x) == 0.0f;
        }
        else
        {
            _S19 = false;
        }
        if(_S19)
        {
            var coord_4 : vec2<f32> = coord_3 - vec2<f32>(2.0f, 0.0f) * params_0.inv_source_0;
            e_7 = sample_edges_0(coord_3).xy;
            coord_3 = coord_4;
        }
        i_2 = i_2 + i32(1);
    }
    return params_0.inv_source_0.x * (-2.0078740119934082f * search_length_0(e_7, 0.0f) + 3.25f) + coord_3.x;
}

fn search_x_right_0( uv_7 : vec2<f32>) -> f32
{
    var e_8 : vec2<f32> = vec2<f32>(0.0f, 1.0f);
    var i_3 : i32 = i32(0);
    var coord_5 : vec2<f32> = uv_7;
    for(;;)
    {
        if(i_3 < i32(16))
        {
        }
        else
        {
            break;
        }
        var _S20 : bool;
        if((e_8.y) > 0.82810002565383911f)
        {
            _S20 = (e_8.x) == 0.0f;
        }
        else
        {
            _S20 = false;
        }
        if(_S20)
        {
            var coord_6 : vec2<f32> = coord_5 + vec2<f32>(2.0f, 0.0f) * params_0.inv_source_0;
            e_8 = sample_edges_0(coord_5).xy;
            coord_5 = coord_6;
        }
        i_3 = i_3 + i32(1);
    }
    return - params_0.inv_source_0.x * (-2.0078740119934082f * search_length_0(e_8, 0.5f) + 3.25f) + coord_5.x;
}

fn area_ortho_0( dist_1 : vec2<f32>,  e1_0 : f32,  e2_0 : f32) -> vec2<f32>
{
    return (textureSampleLevel((area_0), (tableSampler_0), (vec2<f32>(0.00625000009313226f, 0.01250000018626451f) * (vec2<f32>(16.0f, 16.0f) * round(vec2<f32>(4.0f) * vec2<f32>(e1_0, e2_0)) + dist_1) + vec2<f32>(0.5f) * vec2<f32>(0.00625000009313226f, 0.01250000018626451f)), (0.0f))).xy;
}

fn horizontal_corner_factor_0( texcoord_1 : vec4<f32>,  d_1 : vec2<f32>) -> vec2<f32>
{
    var left_right_0 : vec2<f32> = step(d_1.xy, d_1.yx);
    var rounding_0 : vec2<f32> = vec2<f32>(0.75f) * left_right_0 / vec2<f32>((left_right_0.x + left_right_0.y));
    const _S21 : vec2<f32> = vec2<f32>(1.0f, 1.0f);
    var factor_0 : vec2<f32> = _S21;
    var _S22 : f32 = rounding_0.x;
    var _S23 : vec2<f32> = texcoord_1.xy;
    var _S24 : f32 = rounding_0.y;
    var _S25 : vec2<f32> = texcoord_1.zw;
    factor_0[i32(0)] = factor_0[i32(0)] - _S22 * sample_edges_at_0(_S23, vec2<f32>(0.0f, 1.0f)).x - _S24 * sample_edges_at_0(_S25, _S21).x;
    factor_0[i32(1)] = factor_0[i32(1)] - _S22 * sample_edges_at_0(_S23, vec2<f32>(0.0f, -2.0f)).x - _S24 * sample_edges_at_0(_S25, vec2<f32>(1.0f, -2.0f)).x;
    return saturate(factor_0);
}

fn search_y_up_0( uv_8 : vec2<f32>) -> f32
{
    var e_9 : vec2<f32> = vec2<f32>(1.0f, 0.0f);
    var i_4 : i32 = i32(0);
    var coord_7 : vec2<f32> = uv_8;
    for(;;)
    {
        if(i_4 < i32(16))
        {
        }
        else
        {
            break;
        }
        var _S26 : bool;
        if((e_9.x) > 0.82810002565383911f)
        {
            _S26 = (e_9.y) == 0.0f;
        }
        else
        {
            _S26 = false;
        }
        if(_S26)
        {
            var coord_8 : vec2<f32> = coord_7 - vec2<f32>(0.0f, 2.0f) * params_0.inv_source_0;
            e_9 = sample_edges_0(coord_7).xy;
            coord_7 = coord_8;
        }
        i_4 = i_4 + i32(1);
    }
    return params_0.inv_source_0.y * (-2.0078740119934082f * search_length_0(vec2<f32>(e_9.y, e_9.x), 0.0f) + 3.25f) + coord_7.y;
}

fn search_y_down_0( uv_9 : vec2<f32>) -> f32
{
    var e_10 : vec2<f32> = vec2<f32>(1.0f, 0.0f);
    var i_5 : i32 = i32(0);
    var coord_9 : vec2<f32> = uv_9;
    for(;;)
    {
        if(i_5 < i32(16))
        {
        }
        else
        {
            break;
        }
        var _S27 : bool;
        if((e_10.x) > 0.82810002565383911f)
        {
            _S27 = (e_10.y) == 0.0f;
        }
        else
        {
            _S27 = false;
        }
        if(_S27)
        {
            var coord_10 : vec2<f32> = coord_9 + vec2<f32>(0.0f, 2.0f) * params_0.inv_source_0;
            e_10 = sample_edges_0(coord_9).xy;
            coord_9 = coord_10;
        }
        i_5 = i_5 + i32(1);
    }
    return - params_0.inv_source_0.y * (-2.0078740119934082f * search_length_0(vec2<f32>(e_10.y, e_10.x), 0.5f) + 3.25f) + coord_9.y;
}

fn vertical_corner_factor_0( texcoord_2 : vec4<f32>,  d_2 : vec2<f32>) -> vec2<f32>
{
    var left_right_1 : vec2<f32> = step(d_2.xy, d_2.yx);
    var rounding_1 : vec2<f32> = vec2<f32>(0.75f) * left_right_1 / vec2<f32>((left_right_1.x + left_right_1.y));
    const _S28 : vec2<f32> = vec2<f32>(1.0f, 1.0f);
    var factor_1 : vec2<f32> = _S28;
    var _S29 : f32 = rounding_1.x;
    var _S30 : vec2<f32> = texcoord_2.xy;
    var _S31 : f32 = rounding_1.y;
    var _S32 : vec2<f32> = texcoord_2.zw;
    factor_1[i32(0)] = factor_1[i32(0)] - _S29 * sample_edges_at_0(_S30, vec2<f32>(1.0f, 0.0f)).y - _S31 * sample_edges_at_0(_S32, _S28).y;
    factor_1[i32(1)] = factor_1[i32(1)] - _S29 * sample_edges_at_0(_S30, vec2<f32>(-2.0f, 0.0f)).y - _S31 * sample_edges_at_0(_S32, vec2<f32>(-2.0f, 1.0f)).y;
    return saturate(factor_1);
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_10 : vec2<f32>,
};

@fragment
fn fragmentMain( _S33 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var pixcoord_0 : vec2<f32> = _S33.uv_10 * params_0.source_size_0;
    var _S34 : vec4<f32> = vec4<f32>(params_0.inv_source_0, params_0.inv_source_0);
    var _S35 : vec4<f32> = vec4<f32>(_S33.uv_10, _S33.uv_10);
    var offset_h_0 : vec4<f32> = vec4<f32>(-0.25f, -0.125f, 1.25f, -0.125f) * _S34 + _S35;
    var offset_v_0 : vec4<f32> = vec4<f32>(-0.125f, -0.25f, -0.125f, 1.25f) * _S34 + _S35;
    var weights_2 : vec4<f32> = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
    var _S36 : vec2<f32> = sample_edges_0(_S33.uv_10).xy;
    var e_11 : vec2<f32> = _S36;
    if((_S36.y) > 0.0f)
    {
        var diagonal_0 : vec2<f32> = calculate_diag_weights_0(_S33.uv_10, e_11);
        weights_2[i32(0)] = diagonal_0.x;
        weights_2[i32(1)] = diagonal_0.y;
        if((weights_2.x) == (- weights_2.y))
        {
            var coords_2 : vec3<f32>;
            coords_2[i32(0)] = search_x_left_0(offset_h_0.xy);
            coords_2[i32(1)] = offset_v_0.y;
            var d_3 : vec2<f32>;
            d_3[i32(0)] = coords_2.x;
            var e1_1 : f32 = sample_edges_0(coords_2.xy).x;
            coords_2[i32(2)] = search_x_right_0(offset_h_0.zw);
            d_3[i32(1)] = coords_2.z;
            var _S37 : vec2<f32> = abs(round(params_0.source_size_0.xx * d_3 - pixcoord_0.xx));
            d_3 = _S37;
            var found_2 : vec2<f32> = area_ortho_0(sqrt(_S37), e1_1, sample_edges_at_0(vec2<f32>(coords_2.z, coords_2.y), vec2<f32>(1.0f, 0.0f)).x);
            coords_2[i32(1)] = _S33.uv_10.y;
            var found_3 : vec2<f32> = found_2 * horizontal_corner_factor_0(vec4<f32>(coords_2.x, coords_2.y, coords_2.z, coords_2.y), _S37);
            weights_2[i32(0)] = found_3.x;
            weights_2[i32(1)] = found_3.y;
        }
        else
        {
            e_11[i32(0)] = 0.0f;
        }
    }
    if((e_11.x) > 0.0f)
    {
        var coords_3 : vec3<f32>;
        coords_3[i32(1)] = search_y_up_0(offset_v_0.xy);
        coords_3[i32(0)] = offset_h_0.x;
        var d_4 : vec2<f32>;
        d_4[i32(0)] = coords_3.y;
        var e1_2 : f32 = sample_edges_0(coords_3.xy).y;
        coords_3[i32(2)] = search_y_down_0(offset_v_0.zw);
        d_4[i32(1)] = coords_3.z;
        var _S38 : vec2<f32> = abs(round(params_0.source_size_0.yy * d_4 - pixcoord_0.yy));
        d_4 = _S38;
        var found_4 : vec2<f32> = area_ortho_0(sqrt(_S38), e1_2, sample_edges_at_0(vec2<f32>(coords_3.x, coords_3.z), vec2<f32>(0.0f, 1.0f)).y);
        coords_3[i32(0)] = _S33.uv_10.x;
        var found_5 : vec2<f32> = found_4 * vertical_corner_factor_0(vec4<f32>(coords_3.x, coords_3.y, coords_3.x, coords_3.z), _S38);
        weights_2[i32(2)] = found_5.x;
        weights_2[i32(3)] = found_5.y;
    }
    var _S39 : pixelOutput_0 = pixelOutput_0( weights_2 );
    return _S39;
}

